//! VNC session management
//!
//! Wraps vnc-rs to provide a VNC client session with framebuffer management
//! and input forwarding. Includes Apple Remote Desktop (ARD) authentication
//! support for macOS Screen Sharing.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use image::ImageFormat;
use parking_lot::Mutex;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use vnc::client::VncClient;
use vnc::{PixelFormat, VncEncoding, VncEvent, X11Event};

use crate::config::DetectedOs;
use crate::config::settings::{VncEncodingPreference, VncSettings};

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_private_ipv4(v4),
        IpAddr::V6(v6) => is_private_ipv6(v6),
    }
}

fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_private() || ip.is_loopback() || ip.is_link_local()
}

fn is_private_ipv6(ip: Ipv6Addr) -> bool {
    ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local()
}

fn build_encodings(
    preference: VncEncodingPreference,
    is_private: bool,
    allow_tight: bool,
    include_cursor: bool,
) -> Vec<VncEncoding> {
    let mut encodings = Vec::new();

    // Pseudo-encodings for better behavior (cursor + desktop size updates)
    if include_cursor {
        encodings.push(VncEncoding::CursorPseudo);
    }
    encodings.push(VncEncoding::DesktopSizePseudo);

    let mut order = match preference {
        VncEncodingPreference::Auto => {
            if is_private {
                vec![
                    VncEncoding::Zrle,
                    VncEncoding::Tight,
                    VncEncoding::CopyRect,
                    VncEncoding::Raw,
                ]
            } else {
                vec![
                    VncEncoding::Tight,
                    VncEncoding::Zrle,
                    VncEncoding::CopyRect,
                    VncEncoding::Raw,
                ]
            }
        }
        VncEncodingPreference::Tight => vec![
            VncEncoding::Tight,
            VncEncoding::Zrle,
            VncEncoding::CopyRect,
            VncEncoding::Raw,
        ],
        VncEncodingPreference::Zrle => vec![
            VncEncoding::Zrle,
            VncEncoding::Tight,
            VncEncoding::CopyRect,
            VncEncoding::Raw,
        ],
        VncEncodingPreference::Raw => vec![
            VncEncoding::Raw,
            VncEncoding::Zrle,
            VncEncoding::Tight,
            VncEncoding::CopyRect,
        ],
    };

    if !allow_tight {
        order.retain(|encoding| *encoding != VncEncoding::Tight);
    }

    for encoding in order {
        if !encodings.contains(&encoding) {
            encodings.push(encoding);
        }
    }

    if !encodings.contains(&VncEncoding::Raw) {
        encodings.push(VncEncoding::Raw);
    }

    encodings
}

fn pixel_format_from_depth(depth: u8) -> PixelFormat {
    match depth {
        16 => {
            let mut pf = PixelFormat::bgra();
            pf.bits_per_pixel = 16;
            pf.depth = 16;
            pf.big_endian_flag = 0;
            pf.true_color_flag = 1;
            pf.red_max = 31;
            pf.green_max = 63;
            pf.blue_max = 31;
            pf.red_shift = 11;
            pf.green_shift = 5;
            pf.blue_shift = 0;
            pf
        }
        _ => PixelFormat::bgra(),
    }
}
/// Framebuffer holding the current VNC screen contents (BGRA pixels)
#[derive(Debug)]
pub struct FrameBuffer {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub dirty: Option<DirtyRect>,
}

#[derive(Debug, Clone, Copy)]
pub struct DirtyRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

fn rgba_to_bgra(mut data: Vec<u8>) -> Vec<u8> {
    for chunk in data.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }
    data
}

impl FrameBuffer {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
            dirty: Some(DirtyRect {
                x: 0,
                y: 0,
                width,
                height,
            }),
        }
    }

    fn mark_dirty(&mut self, x: u32, y: u32, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }

        let new_rect = DirtyRect {
            x,
            y,
            width: w,
            height: h,
        };

        self.dirty = Some(match self.dirty {
            None => new_rect,
            Some(existing) => {
                let x1 = existing.x.min(new_rect.x);
                let y1 = existing.y.min(new_rect.y);
                let x2 = (existing.x + existing.width).max(new_rect.x + new_rect.width);
                let y2 = (existing.y + existing.height).max(new_rect.y + new_rect.height);
                DirtyRect {
                    x: x1,
                    y: y1,
                    width: x2.saturating_sub(x1),
                    height: y2.saturating_sub(y1),
                }
            }
        });
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
        self.mark_dirty(x, y, w, h);
    }

    /// Apply a 16-bit RGB565 image update to the framebuffer (little-endian unless big_endian is true)
    pub fn apply_raw_565(&mut self, x: u32, y: u32, w: u32, h: u32, data: &[u8], big_endian: bool) {
        let stride = self.width as usize * 4;
        let row_bytes = w as usize * 2;
        for row in 0..h as usize {
            let src_offset = row * row_bytes;
            let dst_offset = (y as usize + row) * stride + x as usize * 4;
            if src_offset + row_bytes > data.len() || dst_offset >= self.pixels.len() {
                break;
            }
            let mut dst = dst_offset;
            for col in 0..w as usize {
                let idx = src_offset + col * 2;
                if idx + 1 >= data.len() || dst + 3 >= self.pixels.len() {
                    break;
                }
                let pixel = if big_endian {
                    u16::from_be_bytes([data[idx], data[idx + 1]])
                } else {
                    u16::from_le_bytes([data[idx], data[idx + 1]])
                };
                let r5 = (pixel >> 11) & 0x1f;
                let g6 = (pixel >> 5) & 0x3f;
                let b5 = pixel & 0x1f;
                let r8 = ((r5 << 3) | (r5 >> 2)) as u8;
                let g8 = ((g6 << 2) | (g6 >> 4)) as u8;
                let b8 = ((b5 << 3) | (b5 >> 2)) as u8;
                self.pixels[dst] = b8;
                self.pixels[dst + 1] = g8;
                self.pixels[dst + 2] = r8;
                self.pixels[dst + 3] = 255;
                dst += 4;
            }
        }
        self.mark_dirty(x, y, w, h);
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
        self.mark_dirty(dst_x, dst_y, w, h);
    }

    /// Resize the framebuffer
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.pixels = vec![0; (width * height * 4) as usize];
        self.dirty = Some(DirtyRect {
            x: 0,
            y: 0,
            width,
            height,
        });
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
    /// Returns the negotiated stream and an optional detected OS based on RFB handshake signals.
    async fn negotiate(
        mut tcp: TcpStream,
        username: &str,
        password: &str,
    ) -> Result<(Self, Option<DetectedOs>), String> {
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
            return Ok((
                Self {
                    inner: tcp,
                    prefix: server_version.to_vec(),
                    prefix_offset: 0,
                    drop_write_bytes: 0,
                },
                None,
            ));
        }

        // Non-standard RFB version (e.g. macOS 003.889) is a strong macOS signal
        let has_nonstandard_version = true;

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
            // Non-standard version still suggests macOS Screen Sharing
            let detected_os = if has_nonstandard_version {
                Some(DetectedOs::MacOS)
            } else {
                None
            };
            let mut prefix = b"RFB 003.008\n".to_vec();
            prefix.push(num_types[0]);
            prefix.extend_from_slice(&types);
            return Ok((
                Self {
                    inner: tcp,
                    prefix,
                    prefix_offset: 0,
                    // vnc-rs will write: 12 bytes version response (goes to server, fine)
                    drop_write_bytes: 0,
                },
                detected_os,
            ));
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
            // ARD (type 30) is macOS-only
            return Ok((
                Self {
                    inner: tcp,
                    prefix,
                    prefix_offset: 0,
                    drop_write_bytes: 13,
                },
                Some(DetectedOs::MacOS),
            ));
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
    /// Minimum interval between pointer events
    mouse_rate_limit: Duration,
    /// Last time a pointer event was sent
    last_mouse_sent: Mutex<Instant>,
    /// Enable verbose VNC logs
    debug_logs: bool,
    /// Minimum interval between resize requests
    resize_rate_limit: Duration,
    /// Last time a resize was requested
    last_resize_sent: Mutex<Instant>,
    /// Last requested desktop size
    last_resize_size: Mutex<(u16, u16)>,
    /// Minimum interval between refresh requests
    refresh_rate_limit: Duration,
    /// Last time a refresh was requested
    last_refresh_sent: Mutex<Instant>,
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
        vnc_settings: VncSettings,
    ) -> Result<
        (
            Arc<Self>,
            mpsc::Receiver<VncSessionEvent>,
            Option<DetectedOs>,
        ),
        String,
    > {
        let debug_logs = std::env::var_os("PORTAL_VNC_DEBUG").is_some();
        tracing::info!(
            "VNC connect to {}:{} (user set: {}, debug_logs: {})",
            hostname,
            port,
            username.as_ref().map(|u| !u.is_empty()).unwrap_or(false),
            debug_logs
        );
        let addr = format!("{}:{}", hostname, port);
        let tcp = TcpStream::connect(&addr)
            .await
            .map_err(|e| format!("Failed to connect to {}: {}", addr, e))?;
        let is_private = tcp
            .peer_addr()
            .ok()
            .map(|addr| is_private_ip(addr.ip()))
            .unwrap_or(false);

        let user = username.as_deref().unwrap_or("");
        let pass = password.as_deref().unwrap_or("");

        // Handle version negotiation + auth (including macOS ARD)
        let (stream, detected_os) = NegotiatedStream::negotiate(tcp, user, pass).await?;

        let pw = password.clone().unwrap_or_default();
        let pixel_format = pixel_format_from_depth(vnc_settings.color_depth);
        let allow_tight = pixel_format.bits_per_pixel == 32;
        let include_cursor = pixel_format.bits_per_pixel == 32;
        let encodings = build_encodings(
            vnc_settings.encoding,
            is_private,
            allow_tight,
            include_cursor,
        );
        tracing::info!(
            "VNC encodings: {:?} (preference: {:?}, private: {}, cursor: {})",
            encodings,
            vnc_settings.encoding,
            is_private,
            include_cursor
        );
        tracing::info!(
            "VNC pixel format: {} bpp (allow_tight: {})",
            pixel_format.bits_per_pixel,
            allow_tight
        );

        let mut connector = vnc::client::VncConnector::new(stream)
            .set_auth_method(async move { Ok(pw) })
            .allow_shared(true)
            .set_pixel_format(pixel_format);
        for encoding in encodings {
            connector = connector.add_encoding(encoding);
        }
        let vnc = connector
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

        let now = Instant::now();
        let session = Arc::new(Self {
            framebuffer: framebuffer.clone(),
            input_tx,
            host_name,
            mouse_rate_limit: Duration::from_millis(vnc_settings.pointer_interval_ms),
            last_mouse_sent: Mutex::new(now),
            debug_logs,
            resize_rate_limit: Duration::from_millis(100),
            last_resize_sent: Mutex::new(now - Duration::from_millis(500)),
            last_resize_size: Mutex::new((0, 0)),
            refresh_rate_limit: Duration::from_millis(100),
            last_refresh_sent: Mutex::new(now - Duration::from_millis(500)),
        });

        // Spawn the VNC event polling loop
        tokio::spawn(Self::event_loop(
            vnc,
            framebuffer,
            event_tx,
            input_rx,
            debug_logs,
            pixel_format,
            vnc_settings.refresh_fps,
            vnc_settings.max_events_per_tick,
        ));

        Ok((session, event_rx, detected_os))
    }

    /// Background task: poll VNC events and forward input
    ///
    /// Uses three separate tasks so that input forwarding, refresh requests,
    /// and event receiving don't contend on the VncClient's internal async
    /// mutex. `recv_event()` blocks (yields) until an event is available,
    /// which naturally releases the mutex for the other tasks.
    #[allow(clippy::too_many_arguments)]
    async fn event_loop(
        vnc: VncClient,
        framebuffer: Arc<Mutex<FrameBuffer>>,
        event_tx: mpsc::Sender<VncSessionEvent>,
        mut input_rx: mpsc::Receiver<X11Event>,
        debug_logs: bool,
        pixel_format: PixelFormat,
        refresh_fps: u32,
        _max_events_per_tick: usize,
    ) {
        let refresh_ms = if refresh_fps == 0 {
            100
        } else {
            (1000u64 / refresh_fps.min(60) as u64).max(1)
        };
        let raw_bpp = (pixel_format.bits_per_pixel / 8) as usize;
        let raw_big_endian = pixel_format.big_endian_flag > 0;

        let vnc = Arc::new(vnc);

        // Spawn input forwarding task — has its own access to vnc mutex
        let vnc_input = vnc.clone();
        let input_handle = tokio::spawn(async move {
            while let Some(event) = input_rx.recv().await {
                if vnc_input.input(event).await.is_err() {
                    break;
                }
            }
        });

        // Shared timestamp: updated by the recv loop each time an event arrives.
        // The refresh task checks this to avoid sending refresh requests while
        // the server is already actively streaming updates (macOS VNC sends
        // full-screen updates per request regardless of actual changes).
        let last_event_at = Arc::new(Mutex::new(Instant::now()));
        let last_event_at_refresh = last_event_at.clone();

        // Spawn refresh task — only sends refresh requests when the event
        // stream has been idle, avoiding redundant full-screen redraws
        let vnc_refresh = vnc.clone();
        let refresh_handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_millis(refresh_ms));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let idle_threshold = Duration::from_millis(refresh_ms.max(100));
            loop {
                interval.tick().await;
                let since_last = last_event_at_refresh.lock().elapsed();
                if since_last >= idle_threshold
                    && vnc_refresh.input(X11Event::Refresh).await.is_err()
                {
                    break;
                }
            }
        });

        // Optional stats logging
        let mut stats_events: u64 = 0;
        let mut stats_raw_rects: u64 = 0;
        let mut stats_copy_rects: u64 = 0;
        let mut stats_jpeg_rects: u64 = 0;
        let mut stats_bytes_raw: u64 = 0;
        let mut stats_bytes_jpeg: u64 = 0;
        let mut stats_apply_raw_ms: u64 = 0;
        let mut stats_jpeg_decode_ms: u64 = 0;
        let mut stats_last_update = Instant::now();
        let mut stats_print_at = Instant::now() + Duration::from_secs(2);

        // Main event loop — recv_event() blocks (yields) until a VNC event is
        // available, which releases the VncClient async mutex so the input and
        // refresh tasks can make progress concurrently.
        loop {
            let event = match vnc.recv_event().await {
                Ok(event) => {
                    *last_event_at.lock() = Instant::now();
                    event
                }
                Err(e) => {
                    tracing::error!("VNC event error: {}", e);
                    let _ = event_tx.send(VncSessionEvent::Disconnected).await;
                    break;
                }
            };

            match event {
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
                    if debug_logs {
                        stats_events += 1;
                        stats_last_update = Instant::now();
                    }
                }
                VncEvent::RawImage(rect, data) => {
                    let t0 = Instant::now();
                    {
                        let mut fb = framebuffer.lock();
                        if raw_bpp == 2 {
                            fb.apply_raw_565(
                                rect.x as u32,
                                rect.y as u32,
                                rect.width as u32,
                                rect.height as u32,
                                &data,
                                raw_big_endian,
                            );
                        } else {
                            fb.apply_raw(
                                rect.x as u32,
                                rect.y as u32,
                                rect.width as u32,
                                rect.height as u32,
                                &data,
                            );
                        }
                    }
                    if debug_logs {
                        let dt = t0.elapsed();
                        stats_apply_raw_ms += dt.as_millis() as u64;
                        stats_events += 1;
                        stats_raw_rects += 1;
                        stats_bytes_raw += data.len() as u64;
                        stats_last_update = Instant::now();
                    }
                }
                VncEvent::JpegImage(rect, data) => {
                    let decode_start = Instant::now();
                    let jpeg_len = data.len();
                    let rx = rect.x as u32;
                    let ry = rect.y as u32;
                    let rw = rect.width;
                    let rh = rect.height;
                    // Decode JPEG on a blocking thread to avoid stalling
                    // the async event loop (and thus input/refresh tasks)
                    let fb_ref = framebuffer.clone();
                    match tokio::task::spawn_blocking(move || {
                        let img = image::load_from_memory_with_format(&data, ImageFormat::Jpeg)?;
                        let rgba = img.to_rgba8();
                        let (w, h) = rgba.dimensions();
                        let pixels = rgba_to_bgra(rgba.into_raw());
                        // Apply directly inside the blocking task to avoid
                        // round-tripping the decoded pixels back
                        let mut fb = fb_ref.lock();
                        if w != rw as u32 || h != rh as u32 {
                            tracing::warn!(
                                "VNC JPEG dims {}x{} do not match rect {}x{}",
                                w,
                                h,
                                rw,
                                rh
                            );
                        }
                        fb.apply_raw(rx, ry, w, h, &pixels);
                        Ok::<_, image::ImageError>(())
                    })
                    .await
                    {
                        Ok(Ok(())) => {
                            if debug_logs {
                                let dt = decode_start.elapsed();
                                stats_jpeg_decode_ms += dt.as_millis() as u64;
                                stats_events += 1;
                                stats_jpeg_rects += 1;
                                stats_bytes_jpeg += jpeg_len as u64;
                                stats_last_update = Instant::now();
                            }
                        }
                        Ok(Err(e)) => {
                            tracing::error!("VNC JPEG decode error: {}", e);
                        }
                        Err(e) => {
                            tracing::error!("VNC JPEG decode task panicked: {}", e);
                        }
                    }
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
                    if debug_logs {
                        stats_events += 1;
                        stats_copy_rects += 1;
                        stats_last_update = Instant::now();
                    }
                }
                VncEvent::Bell => {
                    let _ = event_tx.send(VncSessionEvent::Bell).await;
                    if debug_logs {
                        stats_events += 1;
                    }
                }
                VncEvent::SetCursor(_rect, _data) => {}
                VncEvent::Text(_text) => {}
                _ => {}
            }

            // Periodic stats logging
            if debug_logs {
                let now = Instant::now();
                if now >= stats_print_at {
                    let since_update = stats_last_update.elapsed().as_millis();
                    tracing::debug!(
                        "VNC stats: events={} raw_rects={} copy_rects={} jpeg_rects={} bytes_raw={} bytes_jpeg={} apply_raw_ms={} jpeg_decode_ms={} last_update_ms={}",
                        stats_events,
                        stats_raw_rects,
                        stats_copy_rects,
                        stats_jpeg_rects,
                        stats_bytes_raw,
                        stats_bytes_jpeg,
                        stats_apply_raw_ms,
                        stats_jpeg_decode_ms,
                        since_update,
                    );
                    stats_events = 0;
                    stats_raw_rects = 0;
                    stats_copy_rects = 0;
                    stats_jpeg_rects = 0;
                    stats_bytes_raw = 0;
                    stats_bytes_jpeg = 0;
                    stats_apply_raw_ms = 0;
                    stats_jpeg_decode_ms = 0;
                    stats_print_at = now + Duration::from_secs(2);
                }
            }
        }

        input_handle.abort();
        refresh_handle.abort();
    }

    /// Send a mouse event to the VNC server
    pub async fn send_mouse(&self, x: u16, y: u16, buttons: u8) {
        if self.mouse_rate_limit > Duration::ZERO {
            let now = Instant::now();
            let mut last = self.last_mouse_sent.lock();
            if now.duration_since(*last) < self.mouse_rate_limit {
                return;
            }
            *last = now;
        }
        let _ = self
            .input_tx
            .send(X11Event::PointerEvent((x, y, buttons).into()))
            .await;
    }

    /// Request a remote desktop resize (best-effort)
    pub fn try_request_desktop_size(&self, width: u16, height: u16) {
        if width == 0 || height == 0 {
            return;
        }

        let now = Instant::now();
        let mut last_sent = self.last_resize_sent.lock();
        if now.duration_since(*last_sent) < self.resize_rate_limit {
            return;
        }

        let mut last_size = self.last_resize_size.lock();
        if *last_size == (width, height) {
            return;
        }

        *last_sent = now;
        *last_size = (width, height);
        if let Err(err) = self
            .input_tx
            .try_send(X11Event::SetDesktopSize { width, height })
        {
            if self.debug_logs {
                tracing::debug!("VNC resize request dropped: {}", err);
            }
        } else if self.debug_logs {
            tracing::debug!("VNC resize requested: {}x{}", width, height);
        }
    }

    /// Request a framebuffer refresh (best-effort, rate-limited)
    pub fn try_request_refresh(&self) {
        let now = Instant::now();
        let mut last = self.last_refresh_sent.lock();
        if now.duration_since(*last) < self.refresh_rate_limit {
            return;
        }
        *last = now;
        if let Err(err) = self.input_tx.try_send(X11Event::Refresh) {
            if self.debug_logs {
                tracing::debug!("VNC refresh request dropped: {}", err);
            }
        } else if self.debug_logs {
            tracing::debug!("VNC refresh requested");
        }
    }

    /// Send a key event to the VNC server (async)
    pub async fn send_key(&self, keysym: u32, pressed: bool) {
        if let Err(err) = self
            .input_tx
            .send(X11Event::KeyEvent((keysym, pressed).into()))
            .await
        {
            if self.debug_logs {
                tracing::debug!("VNC key event send failed: {}", err);
            }
        }
    }

    /// Send a key event synchronously (non-blocking, for use from update handlers)
    pub fn try_send_key(&self, keysym: u32, pressed: bool) {
        if let Err(err) = self
            .input_tx
            .try_send(X11Event::KeyEvent((keysym, pressed).into()))
        {
            if self.debug_logs {
                tracing::debug!("VNC key event dropped: {}", err);
            }
        }
    }

    /// Disconnect the session
    pub fn disconnect(&self) {
        // Dropping the input_tx will cause the input forwarding task to end,
        // which will cascade to close the VNC connection
    }
}
