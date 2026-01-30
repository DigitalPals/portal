//! VNC session management
//!
//! Wraps vnc-rs to provide a VNC client session with framebuffer management
//! and input forwarding.

use std::sync::Arc;

use parking_lot::Mutex;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use vnc::client::VncClient;
use vnc::{PixelFormat, VncEncoding, VncEvent, X11Event};

/// Framebuffer holding the current VNC screen contents (BGRA pixels)
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
        // Copy row by row, handling overlap
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
    /// Framebuffer has been updated, UI should re-render
    FrameUpdated,
    /// Resolution changed
    ResolutionChanged(u32, u32),
    /// Connection was closed
    Disconnected,
    /// Bell notification
    Bell,
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
        password: Option<String>,
        host_name: String,
    ) -> Result<(Arc<Self>, mpsc::Receiver<VncSessionEvent>), String> {
        let addr = format!("{}:{}", hostname, port);
        let tcp = TcpStream::connect(&addr)
            .await
            .map_err(|e| format!("Failed to connect to {}: {}", addr, e))?;

        let vnc = vnc::client::VncConnector::new(tcp)
            .set_auth_method(async move { Ok(password.unwrap_or_default()) })
            .add_encoding(VncEncoding::Tight)
            .add_encoding(VncEncoding::Zrle)
            .add_encoding(VncEncoding::CopyRect)
            .add_encoding(VncEncoding::Raw)
            .allow_shared(true)
            .set_pixel_format(PixelFormat::bgra())
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

        // Main event polling loop
        loop {
            match vnc.poll_event().await {
                Ok(Some(event)) => match event {
                    VncEvent::SetResolution(screen) => {
                        let w = screen.width as u32;
                        let h = screen.height as u32;
                        framebuffer.lock().resize(w, h);
                        let _ = event_tx.send(VncSessionEvent::ResolutionChanged(w, h)).await;
                    }
                    VncEvent::RawImage(rect, data) => {
                        framebuffer.lock().apply_raw(
                            rect.x as u32,
                            rect.y as u32,
                            rect.width as u32,
                            rect.height as u32,
                            &data,
                        );
                        let _ = event_tx.send(VncSessionEvent::FrameUpdated).await;
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
                        let _ = event_tx.send(VncSessionEvent::FrameUpdated).await;
                    }
                    VncEvent::Bell => {
                        let _ = event_tx.send(VncSessionEvent::Bell).await;
                    }
                    VncEvent::SetCursor(_rect, _data) => {
                        // Could implement custom cursor rendering in the future
                    }
                    VncEvent::Text(_text) => {
                        // Clipboard text from server - could forward to system clipboard
                    }
                    _ => {}
                },
                Ok(None) => {
                    // No event available, yield
                    tokio::task::yield_now().await;
                }
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

    /// Send a key event to the VNC server
    pub async fn send_key(&self, keysym: u32, pressed: bool) {
        let _ = self
            .input_tx
            .send(X11Event::KeyEvent((keysym, pressed).into()))
            .await;
    }

    /// Disconnect the session
    pub fn disconnect(&self) {
        // Dropping the input_tx will cause the input forwarding task to end,
        // which will cascade to close the VNC connection
    }
}
