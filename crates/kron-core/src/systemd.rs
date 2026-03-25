/// Notify systemd of daemon state changes via the `sd_notify` protocol.
///
/// Sends a datagram to `$NOTIFY_SOCKET` if set. No-op on non-Unix or when
/// systemd is not supervising this process (e.g., macOS, manual start).
#[allow(unused_variables)]
pub fn sd_notify(state: &str) {
    #[cfg(unix)]
    {
        if let Ok(path) = std::env::var("NOTIFY_SOCKET") {
            use std::os::unix::net::UnixDatagram;
            let _ = UnixDatagram::unbound().and_then(|sock| sock.send_to(state.as_bytes(), &path));
        }
    }
}
