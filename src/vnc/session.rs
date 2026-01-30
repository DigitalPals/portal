//! VNC session management
//!
//! Wraps vnc-rs to provide a VNC client session with framebuffer management
//! and input forwarding. Includes Apple Remote Desktop (ARD) authentication
//! support for macOS Screen Sharing.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use parking_lot::Mutex;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use vnc::client::VncClient;
use vnc::{PixelFormat, VncEncoding, VncEvent, X11Event};

/// Framebuffer holding the current VNC screen contents (RGBA pixels)
#[derive(Debug)]
pub struct FrameBuffer {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl FrameBuffer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
        }
    }

    /// Apply a raw image update to the framebuffer
    pub fn apply_raw(&mut self, x: u32, y: u32, w: u32, h: u32, data: &[u8]) {
        let stride = self.width as usize * 4;
        for row in 0..h as usize {
            let src_offset = row * w as usize * 4;
            let dst_offset = (y as usize + row) * stride + x as usize * 4;
            let len = w as usize * 4;
            if src_offset + len <= data.len() && dst_offset + len <= self.pixels.len() {
                self.pixels[dst_offset..dst_offset + len]
                    .copy_from_slice(&data[src_offset..src_offset + len]);
            }
        }
    }

    /// Apply a copy rect operation
    pub fn apply_copy(&mut self, dst_x: u32, dst_y: u32, src_x: u32, src_y: u32, w: u32, h: u32) {
        let stride = self.width as usize * 4;
        let mut temp = vec![0u8; w as usize * 4];
        for row in 0..h as usize {
            let src_offset = (src_y as usize + row) * stride + src_x as usize * 4;
            let dst_offset = (dst_y as usize + row) * stride + dst_x as usize * 4;
            let len = w as usize * 4;
            if src_offset + len <= self.pixels.len() && dst_offset + len <= self.pixels.len() {
                temp[..len].copy_from_slice(&self.pixels[src_offset..src_offset + len]);
                self.pixels[dst_offset..dst_offset + len].copy_from_slice(&temp[..len]);
            }
        }
    }

    /// Resize the framebuffer
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.pixels = vec![0; (width * height * 4) as usize];
    }
}

/// Events emitted by the VNC session to the UI layer
#[derive(Debug, Clone)]
pub enum VncSessionEvent {
    /// Resolution changed
    ResolutionChanged(u32, u32),
    /// Connection was closed
    Disconnected,
    /// Bell notification
    Bell,
}

// ── Apple Remote Desktop (ARD) authentication ───────────────────────────────
//
// Security type 30. Used by macOS Screen Sharing.
// Protocol:
//   1. Server sends: generator(2) + key_len(2) + prime(key_len) + server_pubkey(key_len)
//   2. Client generates DH keypair, computes shared secret
//   3. Shared secret → MD5 → AES-128-ECB key
//   4. Encrypt 64-byte username + 64-byte password (null-padded)
//   5. Client sends: encrypted_credentials(128) + client_pubkey(key_len)
//   6. Server sends SecurityResult(4)

mod ard {
    use aes::Aes128;
    use aes::cipher::{BlockEncrypt, KeyInit};
    use md5::{Digest, Md5};
    use num_bigint::BigUint;
    use num_traits::One;
    use rand::RngCore;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    pub async fn authenticate(
        tcp: &mut TcpStream,
        username: &str,
        password: &str,
    ) -> Result<(), String> {
        // Read DH parameters from server
        let mut gen_buf = [0u8; 2];
        tcp.read_exact(&mut gen_buf)
            .await
            .map_err(|e| format!("ARD: failed to read generator: {}", e))?;
        let generator = u16::from_be_bytes(gen_buf) as u64;

        let mut len_buf = [0u8; 2];
        tcp.read_exact(&mut len_buf)
            .await
            .map_err(|e| format!("ARD: failed to read key length: {}", e))?;
        let key_len = u16::from_be_bytes(len_buf) as usize;

        tracing::info!("ARD: generator={}, key_len={}", generator, key_len);

        let mut prime_bytes = vec![0u8; key_len];
        tcp.read_exact(&mut prime_bytes)
            .await
            .map_err(|e| format!("ARD: failed to read prime: {}", e))?;

        let mut server_pub_bytes = vec![0u8; key_len];
        tcp.read_exact(&mut server_pub_bytes)
            .await
            .map_err(|e| format!("ARD: failed to read server pubkey: {}", e))?;

        let prime = BigUint::from_bytes_be(&prime_bytes);
        let server_pub = BigUint::from_bytes_be(&server_pub_bytes);
        let dh_gen = BigUint::from(generator);

        // Generate client DH keypair
        let mut private_bytes = vec![0u8; key_len];
        rand::thread_rng().fill_bytes(&mut private_bytes);
        let client_private = BigUint::from_bytes_be(&private_bytes);

        let client_pub = dh_gen.modpow(&client_private, &prime);
        let shared_secret = server_pub.modpow(&client_private, &prime);

        // Derive AES key: MD5 of shared secret (zero-padded to key_len)
        let mut secret_bytes = shared_secret.to_bytes_be();
        // Pad to key_len with leading zeros
        while secret_bytes.len() < key_len {
            secret_bytes.insert(0, 0);
        }

        let mut hasher = Md5::new();
        hasher.update(&secret_bytes);
        let md5_hash = hasher.finalize();

        // Encrypt credentials: 64 bytes username + 64 bytes password (null-padded)
        let mut credentials = [0u8; 128];
        let user_bytes = username.as_bytes();
        let pass_bytes = password.as_bytes();
        let user_len = user_bytes.len().min(63);
        let pass_len = pass_bytes.len().min(63);
        credentials[..user_len].copy_from_slice(&user_bytes[..user_len]);
        credentials[64..64 + pass_len].copy_from_slice(&pass_bytes[..pass_len]);

        // AES-128-ECB encrypt the credentials
        let key = aes::cipher::generic_array::GenericArray::from_slice(md5_hash.as_ref());
        let cipher = Aes128::new(key);
        for chunk in credentials.chunks_exact_mut(16) {
            cipher.encrypt_block(chunk.into());
        }

        // Send: encrypted credentials + client public key (zero-padded to key_len)
        tcp.write_all(&credentials)
            .await
            .map_err(|e| format!("ARD: failed to write credentials: {}", e))?;

        let mut client_pub_bytes = client_pub.to_bytes_be();
        while client_pub_bytes.len() < key_len {
            client_pub_bytes.insert(0, 0);
        }
        // Truncate if somehow longer (shouldn't happen with proper DH)
        if client_pub_bytes.len() > key_len {
            let start = client_pub_bytes.len() - key_len;
            client_pub_bytes = client_pub_bytes[start..].to_vec();
        }
        tcp.write_all(&client_pub_bytes)
            .await
            .map_err(|e| format!("ARD: failed to write client pubkey: {}", e))?;

        // Read SecurityResult
        let mut result = [0u8; 4];
        tcp.read_exact(&mut result)
            .await
            .map_err(|e| format!("ARD: failed to read security result: {}", e))?;

        let result_val = u32::from_be_bytes(result);
        if result_val != 0 {
            return Err("ARD authentication failed (wrong username/password)".to_string());
        }

        tracing::info!("ARD authentication succeeded");
        Ok(())
    }

    /// Ensure the private key is at least 1 and less than prime-1
    #[allow(dead_code)]
    fn clamp_private_key(key: &BigUint, prime: &BigUint) -> BigUint {
        let one = BigUint::one();
        let max = prime - &one;
        if key < &one {
            one
        } else if key > &max {
            max
        } else {
            key.clone()
        }
    }
}

/// A wrapper around TcpStream that handles RFB version negotiation and
/// Apple Remote Desktop authentication before passing the stream to vnc-rs.
///
/// For macOS Screen Sharing (RFB 003.889, security type 30), this wrapper:
/// 1. Negotiates the RFB version on the wire
/// 2. Performs ARD (DH + AES) authentication
/// 3. Feeds vnc-rs a fake "no auth" prefix so it skips directly to init
struct NegotiatedStream {
    inner: TcpStream,
    /// Pre-buffered bytes to feed to the reader
    prefix: Vec<u8>,
    /// How many bytes of the prefix have been consumed
    prefix_offset: usize,
    /// Number of write bytes to silently drop (fake handshake writes from vnc-rs)
    drop_write_bytes: usize,
}

impl NegotiatedStream {
    /// Perform version + auth negotiation, then wrap the stream for vnc-rs.
    async fn negotiate(mut tcp: TcpStream, username: &str, password: &str) -> Result<Self, String> {
        // Read server's 12-byte RFB version string
        let mut server_version = [0u8; 12];
        tcp.read_exact(&mut server_version)
            .await
            .map_err(|e| format!("Failed to read VNC server version: {}", e))?;

        let version_str = String::from_utf8_lossy(&server_version);
        tracing::info!("VNC server version: {:?}", version_str.trim());

        let is_standard = matches!(
            &server_version,
            b"RFB 003.003\n" | b"RFB 003.007\n" | b"RFB 003.008\n"
        );

        if is_standard {
            // Standard version — let vnc-rs handle everything normally
            return Ok(Self {
                inner: tcp,
                prefix: server_version.to_vec(),
                prefix_offset: 0,
                drop_write_bytes: 0,
            });
        }

        // Non-standard version (e.g., macOS 003.889).
        // Echo server's version back to satisfy the server.
        tcp.write_all(&server_version)
            .await
            .map_err(|e| format!("Failed to write VNC version response: {}", e))?;

        // Read security types
        // RFB 3.7+ format: 1 byte count, then count bytes of type IDs
        let mut num_types = [0u8; 1];
        tcp.read_exact(&mut num_types)
            .await
            .map_err(|e| format!("Failed to read security type count: {}", e))?;

        if num_types[0] == 0 {
            // Server sent error — read the reason string
            let mut len_buf = [0u8; 4];
            tcp.read_exact(&mut len_buf)
                .await
                .map_err(|e| format!("Failed to read error length: {}", e))?;
            let reason_len = u32::from_be_bytes(len_buf) as usize;
            let mut reason_buf = vec![0u8; reason_len];
            tcp.read_exact(&mut reason_buf)
                .await
                .map_err(|e| format!("Failed to read error reason: {}", e))?;
            let reason = String::from_utf8_lossy(&reason_buf);
            return Err(format!("VNC server rejected connection: {}", reason));
        }

        let mut types = vec![0u8; num_types[0] as usize];
        tcp.read_exact(&mut types)
            .await
            .map_err(|e| format!("Failed to read security types: {}", e))?;

        tracing::info!("VNC security types offered: {:?}", types);

        // Check if standard types are available (prefer VncAuth=2, then None=1)
        if types.contains(&2) || types.contains(&1) {
            // Standard auth available — feed the security type list to vnc-rs
            // and let it handle auth normally.
            let mut prefix = b"RFB 003.008\n".to_vec();
            prefix.push(num_types[0]);
            prefix.extend_from_slice(&types);
            return Ok(Self {
                inner: tcp,
                prefix,
                prefix_offset: 0,
                // vnc-rs will write: 12 bytes version response (goes to server, fine)
                drop_write_bytes: 0,
            });
        }

        // Apple ARD auth (type 30)
        if types.contains(&30) {
            tracing::info!("Using Apple Remote Desktop (ARD) authentication");

            // Tell server we want type 30
            tcp.write_all(&[30])
                .await
                .map_err(|e| format!("Failed to select ARD security type: {}", e))?;

            // Perform ARD authentication
            ard::authenticate(&mut tcp, username, password).await?;

            // Auth succeeded. Now we need vnc-rs to skip version+auth and go
            // straight to the init phase. Feed it a fake RFB 3.8 handshake
            // that uses SecurityType::None with immediate success:
            //   12 bytes: "RFB 003.008\n" (version)
            //    1 byte:  0x01 (1 security type available)
            //    1 byte:  0x01 (SecurityType::None)
            //    4 bytes: 0x00000000 (SecurityResult: OK)
            let mut prefix = b"RFB 003.008\n".to_vec();
            prefix.push(1); // 1 type
            prefix.push(1); // None
            prefix.extend_from_slice(&[0, 0, 0, 0]); // SecurityResult OK

            // vnc-rs will write back:
            //   12 bytes: version string (we must drop — server doesn't expect it)
            //    1 byte:  security type selection (we must drop)
            // Total: 13 bytes to silently discard
            return Ok(Self {
                inner: tcp,
                prefix,
                prefix_offset: 0,
                drop_write_bytes: 13,
            });
        }

        Err(format!(
            "VNC server only offers unsupported security types: {:?}",
            types
        ))
    }
}

impl AsyncRead for NegotiatedStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();

        // First serve any remaining prefix bytes
        if this.prefix_offset < this.prefix.len() {
            let remaining = &this.prefix[this.prefix_offset..];
            let to_copy = remaining.len().min(buf.remaining());
            buf.put_slice(&remaining[..to_copy]);
            this.prefix_offset += to_copy;
            return Poll::Ready(Ok(()));
        }

        // Then delegate to the inner stream
        Pin::new(&mut this.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for NegotiatedStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();

        // Drop bytes that vnc-rs writes for the fake handshake
        if this.drop_write_bytes > 0 {
            let to_drop = buf.len().min(this.drop_write_bytes);
            this.drop_write_bytes -= to_drop;
            return Poll::Ready(Ok(to_drop));
        }

        Pin::new(&mut this.inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

/// Active VNC session wrapping a vnc-rs client
pub struct VncSession {
    /// Shared framebuffer (updated by background task, read by widget)
    pub framebuffer: Arc<Mutex<FrameBuffer>>,
    /// Channel to send X11 input events to the VNC client
    input_tx: mpsc::Sender<X11Event>,
    /// Hostname for display
    pub host_name: String,
}

impl std::fmt::Debug for VncSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VncSession")
            .field("host_name", &self.host_name)
            .finish()
    }
}

impl VncSession {
    /// Connect to a VNC server and start the event polling loop.
    ///
    /// Returns the session and a receiver for VNC events to forward to the UI.
    pub async fn connect(
        hostname: &str,
        port: u16,
        username: Option<String>,
        password: Option<String>,
        host_name: String,
    ) -> Result<(Arc<Self>, mpsc::Receiver<VncSessionEvent>), String> {
        let addr = format!("{}:{}", hostname, port);
        let tcp = TcpStream::connect(&addr)
            .await
            .map_err(|e| format!("Failed to connect to {}: {}", addr, e))?;

        let user = username.as_deref().unwrap_or("");
        let pass = password.as_deref().unwrap_or("");

        // Handle version negotiation + auth (including macOS ARD)
        let stream = NegotiatedStream::negotiate(tcp, user, pass).await?;

        let pw = password.clone().unwrap_or_default();
        let vnc = vnc::client::VncConnector::new(stream)
            .set_auth_method(async move { Ok(pw) })
            .add_encoding(VncEncoding::Tight)
            .add_encoding(VncEncoding::Zrle)
            .add_encoding(VncEncoding::CopyRect)
            .add_encoding(VncEncoding::Raw)
            .allow_shared(true)
            .set_pixel_format(PixelFormat::rgba())
            .build()
            .map_err(|e| format!("VNC build error: {}", e))?
            .try_start()
            .await
            .map_err(|e| format!("VNC handshake failed: {}", e))?
            .finish()
            .map_err(|e| format!("VNC finish error: {}", e))?;

        // Initial framebuffer - will be resized on first SetResolution event
        let framebuffer = Arc::new(Mutex::new(FrameBuffer::new(800, 600)));

        let (event_tx, event_rx) = mpsc::channel::<VncSessionEvent>(256);
        let (input_tx, input_rx) = mpsc::channel::<X11Event>(256);

        let session = Arc::new(Self {
            framebuffer: framebuffer.clone(),
            input_tx,
            host_name,
        });

        // Spawn the VNC event polling loop
        tokio::spawn(Self::event_loop(vnc, framebuffer, event_tx, input_rx));

        Ok((session, event_rx))
    }

    /// Background task: poll VNC events and forward input
    async fn event_loop(
        vnc: VncClient,
        framebuffer: Arc<Mutex<FrameBuffer>>,
        event_tx: mpsc::Sender<VncSessionEvent>,
        mut input_rx: mpsc::Receiver<X11Event>,
    ) {
        let vnc = Arc::new(vnc);
        let vnc_input = vnc.clone();

        // Spawn input forwarding task
        let input_handle = tokio::spawn(async move {
            while let Some(event) = input_rx.recv().await {
                if vnc_input.input(event).await.is_err() {
                    break;
                }
            }
        });

        // Refresh ticker (~60fps)
        let vnc_refresh = vnc.clone();
        let refresh_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_millis(16));
            loop {
                interval.tick().await;
                if vnc_refresh.input(X11Event::Refresh).await.is_err() {
                    break;
                }
            }
        });

        // Main event loop — use recv_event() which blocks until an event is
        // available. This avoids starving the input/refresh tasks that also need
        // the VncClient's internal mutex lock.
        loop {
            match vnc.recv_event().await {
                Ok(event) => match event {
                    VncEvent::SetResolution(screen) => {
                        let w = screen.width as u32;
                        let h = screen.height as u32;
                        tracing::info!("VNC SetResolution: {}x{}", w, h);
                        {
                            let mut fb = framebuffer.lock();
                            if fb.width != w || fb.height != h {
                                fb.resize(w, h);
                            }
                        }
                        let _ = event_tx
                            .send(VncSessionEvent::ResolutionChanged(w, h))
                            .await;
                    }
                    VncEvent::RawImage(rect, data) => {
                        framebuffer.lock().apply_raw(
                            rect.x as u32,
                            rect.y as u32,
                            rect.width as u32,
                            rect.height as u32,
                            &data,
                        );
                        // Don't send per-frame messages — a timer subscription
                        // handles re-renders at a steady rate
                    }
                    VncEvent::Copy(dst, src) => {
                        framebuffer.lock().apply_copy(
                            dst.x as u32,
                            dst.y as u32,
                            src.x as u32,
                            src.y as u32,
                            dst.width as u32,
                            dst.height as u32,
                        );
                    }
                    VncEvent::Bell => {
                        let _ = event_tx.send(VncSessionEvent::Bell).await;
                    }
                    VncEvent::SetCursor(_rect, _data) => {}
                    VncEvent::Text(_text) => {}
                    _ => {}
                },
                Err(e) => {
                    tracing::error!("VNC event error: {}", e);
                    let _ = event_tx.send(VncSessionEvent::Disconnected).await;
                    break;
                }
            }
        }

        input_handle.abort();
        refresh_handle.abort();
    }

    /// Send a mouse event to the VNC server
    pub async fn send_mouse(&self, x: u16, y: u16, buttons: u8) {
        let _ = self
            .input_tx
            .send(X11Event::PointerEvent((x, y, buttons).into()))
            .await;
    }

    /// Send a key event to the VNC server (async)
    pub async fn send_key(&self, keysym: u32, pressed: bool) {
        let _ = self
            .input_tx
            .send(X11Event::KeyEvent((keysym, pressed).into()))
            .await;
    }

    /// Send a key event synchronously (non-blocking, for use from update handlers)
    pub fn try_send_key(&self, keysym: u32, pressed: bool) {
        let _ = self
            .input_tx
            .try_send(X11Event::KeyEvent((keysym, pressed).into()));
    }

    /// Disconnect the session
    pub fn disconnect(&self) {
        // Dropping the input_tx will cause the input forwarding task to end,
        // which will cascade to close the VNC connection
    }
}
