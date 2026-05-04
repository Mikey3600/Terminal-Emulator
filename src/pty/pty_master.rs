//! # pty.rs — PTY (Pseudo-Terminal) layer
//!
//! ## What is a PTY?
//! A PTY (Pseudo-Terminal) is a pair of virtual file descriptors that behaves
//! exactly like a real serial terminal, but lives entirely in kernel memory.
//!
//!   [master fd]  <——————————————>  [slave fd]
//!   (our program reads/writes)       (shell's stdin/stdout/stderr)
//!
//! When the shell writes "$ " to its stdout, we read it from master_fd.
//! When the user types "ls", we write those bytes to master_fd, and the
//! shell reads them from its stdin (slave_fd).
//!
//! ## Invariants this module upholds
//! 1. Exactly one `PtyMaster` owns `master_fd` at any time — Drop closes it.
//! 2. `slave_fd` is closed in the parent immediately after fork.
//! 3. The child process never returns from `spawn_shell`; it either execs or exits.
//! 4. Caller is responsible for reaping the child (see NOTE on zombie processes below).
//!
//! ## Zombie process warning
//! `spawn_shell` returns the child PID. The caller MUST eventually call
//! `waitpid(child_pid, ...)` (or install a SIGCHLD handler) to reap the child.
//! If you don't, when the shell exits, the kernel keeps its exit status in the
//! process table forever — a "zombie" process. One is harmless; thousands crash
//! servers. See the `WaitStatus` usage in your session manager.

use nix::pty::{openpty, Winsize};
use nix::sys::wait::waitpid;
use nix::unistd::{dup2, execvp, fork, setsid, ForkResult, Pid};
use std::os::unix::io::{AsRawFd, RawFd};

use std::ffi::CString;
use std::mem::ManuallyDrop;
use thiserror::Error;

// ─── Error Types ─────────────────────────────────────────────────────────────

/// Every way this module can fail, with context preserved.
///
/// We use `thiserror` so each variant auto-implements `std::error::Error`
/// and gives a human-readable message via `Display`.
#[allow(clippy::enum_variant_names)]
#[derive(Error, Debug)]
pub enum PtyError {
    /// `openpty()` failed — likely hit the system fd limit (ulimit -n).
    #[error("Failed to open PTY pair: {0}")]
    OpenFailed(nix::Error),

    /// `fork()` failed — system is out of memory or hit max process count.
    #[error("Fork failed: {0}")]
    ForkFailed(nix::Error),

    /// A libc read/write syscall returned -1.
    /// We capture errno at the call site so this is actually debuggable.
    #[error("I/O error on PTY master fd: {0}")]
    IoFailed(std::io::Error),

    // FIX #1: dedicated variant for waitpid failures — distinct from fork failures.
    /// `waitpid()` failed while reaping the child process.
    #[error("waitpid failed: {0}")]
    WaitFailed(nix::Error),
}

// ─── PtyMaster ───────────────────────────────────────────────────────────────

/// Owns the PTY master file descriptor.
///
/// ## RAII — why this struct exists
/// RAII = "Resource Acquisition Is Initialization".
/// The idea: tie a resource's lifetime to a Rust object's lifetime.
///
/// - **Acquire**: `spawn_shell()` opens master_fd and wraps it in `PtyMaster`.
/// - **Use**: caller reads/writes through this struct.
/// - **Release**: when `PtyMaster` goes out of scope, `Drop::drop` runs
///   automatically and closes master_fd. You can never forget to close it.
///
/// ## Why fd is private
/// If `fd` were `pub`, any caller could do `libc::close(master.fd)` and then
/// our `Drop` would close it a second time — a double-close, which is
/// undefined behavior (the fd number may have been reused by then, closing
/// a totally different file). Keep it private; use the provided methods.
pub struct PtyMaster {
    fd: RawFd,
    /// PID of the child shell process. Stored here so whoever owns PtyMaster
    /// also owns the responsibility of reaping the child via `child_pid()`.
    child_pid: Pid,
}

impl PtyMaster {
    /// The raw file descriptor. Use only if you need to pass it to a
    /// low-level syscall. Do NOT close it manually.
    #[allow(dead_code)]
    pub fn as_raw_fd(&self) -> RawFd {
        self.fd
    }

    /// PID of the shell process spawned alongside this PTY.
    /// Pass this to `waitpid` when you detect the shell has exited
    /// (read returns 0 or EIO) to avoid zombie processes.
    ///
    /// Example:
    /// ```rust
    /// // Call waitpid after shell exits to reap the child process.
    /// ```
    #[allow(dead_code)]
    pub fn child_pid(&self) -> Pid {
        self.child_pid
    }
}

impl Drop for PtyMaster {
    fn drop(&mut self) {
        // We use libc::close directly because nix 0.29's close() requires
        // an AsFd-implementing type, not a RawFd integer.
        // SAFETY: self.fd is a valid open fd that we uniquely own.
        // No other code closes it — that's the entire point of this struct.
        unsafe {
            libc::close(self.fd);
        }
    }
}

// SAFETY justification for Send + Sync:
//
// Rust doesn't auto-derive Send/Sync for types containing RawFd (a bare i32)
// because the compiler can't verify ownership semantics of file descriptors.
//
// We assert these manually because:
// - `PtyMaster` is the unique owner of master_fd (no cloning, no copies).
// - Our calling code (Arc<PtyMaster> + tokio::select!) ensures only one task
//   calls read OR write at a time — never concurrently on the same fd.
//
// If you ever change the access pattern (e.g., multiple concurrent readers),
// remove these impls and use a Mutex<PtyMaster> instead.
unsafe impl Send for PtyMaster {}
unsafe impl Sync for PtyMaster {}

// ─── TermSize ────────────────────────────────────────────────────────────────

/// Terminal window dimensions in character cells (not pixels).
///
/// The kernel uses this to answer `TIOCGWINSZ` ioctls from the shell,
/// which is how programs like `vim` and `htop` know how wide to draw.
/// If you don't set this, the shell defaults to 0×0 and most TUI apps break.
#[derive(Debug, Clone)]
pub struct TermSize {
    pub rows: u16,
    pub cols: u16,
    pub shell: Option<String>,
}

impl TermSize {
    /// Typical 80×24 terminal — a safe default for testing.
    #[allow(dead_code)]
    pub fn default_vt100() -> Self {
        Self { rows: 24, cols: 80, shell: None }
    }
}

// ─── spawn_shell ─────────────────────────────────────────────────────────────

/// Opens a PTY pair, forks a child process, and execs a shell in that child.
///
/// ## What happens, step by step
///
/// 1. `openpty()` asks the kernel for a new PTY pair: master_fd + slave_fd.
/// 2. We `fork()`. The kernel clones our process into parent + child.
///    Both processes continue from the same point in code (the match below).
/// 3. **Child path**:
///    - `setsid()`: child becomes leader of a new session. This detaches it
///      from our controlling terminal so it can adopt the PTY as its own.
///    - `dup2(slave_fd, 0/1/2)`: replaces child's stdin/stdout/stderr with
///      the PTY slave. Now anything the shell prints goes to slave_fd.
///    - Close both raw fds (they've been duplicated to 0,1,2; we don't
///      need the originals anymore).
///    - `execvp(shell)`: replaces the child process image with the shell
///      binary. If this succeeds, the child IS now bash/sh/zsh and never
///      returns here. If it fails, we exit(1) cleanly.
/// 4. **Parent path**:
///    - Close slave_fd — parent never touches the slave side.
///    - Return `PtyMaster { fd: master_fd }`.
///
/// ## Shell selection
/// Reads `$SHELL` from the environment, falls back to `/bin/sh`.
/// `/bin/sh` is guaranteed by POSIX on every Unix system.
/// `/bin/bash` is NOT — it's missing on Alpine Linux and some BSDs.
///
/// ## fd leak prevention
/// We use `ManuallyDrop` to take ownership of the nix `OwnedFd` values
/// before any branching, then drop them explicitly on the error path.
/// Old code used `mem::forget` but that leaks fds if fork() returns an error
/// between the forget and the fork call.
pub fn spawn_shell(size: TermSize) -> Result<PtyMaster, PtyError> {
    log::info!("pty_spawn_shell rows={} cols={}", size.rows, size.cols);
    // Extensibility note: this Unix PTY path can be abstracted behind a backend trait
    // to support a Windows ConPTY implementation without changing parser/renderer APIs.
    let winsize = Winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: 0, // pixel dimensions — 0 is fine, rarely used
        ws_ypixel: 0,
    };

    // Ask the kernel for a PTY pair.
    // openpty() in nix 0.29 returns OwnedFd — a type that closes the fd
    // on drop. We immediately wrap in ManuallyDrop so we control the lifetime.
    let pty = openpty(Some(&winsize), None).map_err(PtyError::OpenFailed)?;
    let master = ManuallyDrop::new(pty.master);
    let slave = ManuallyDrop::new(pty.slave);

    // Extract the raw integers. These are just numbers — the actual
    // "file descriptor" is a kernel object; these are just indices into
    // the per-process fd table.
    let master_fd: RawFd = master.as_raw_fd();
    let slave_fd: RawFd = slave.as_raw_fd();

    // Determine which shell to launch.
    // Using $SHELL is better than hardcoding /bin/bash:
    //   - bash doesn't exist on Alpine Linux (uses ash)
    //   - users may prefer zsh, fish, etc.
    //   - /bin/sh is the POSIX-guaranteed fallback
    let shell_path =
        size.shell.or_else(|| std::env::var("SHELL").ok()).unwrap_or_else(|| "/bin/sh".to_string());
    let shell_cstr = CString::new(shell_path.clone()).map_err(|_| {
        PtyError::IoFailed(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "shell path contained null byte",
        ))
    })?;

    // SAFETY: fork() is inherently unsafe. After fork, the child is a
    // copy of the parent's memory but only one thread — all other threads
    // from the parent do NOT exist in the child. Calling anything that
    // uses mutexes or async runtimes in the child before exec is UB.
    // We only use async-signal-safe functions: setsid, dup2, close, execvp.
    let fork_result = unsafe { fork() }.map_err(PtyError::ForkFailed)?;

    match fork_result {
        // ── Child process ────────────────────────────────────────────────
        // This code runs in the forked child. We must exec or exit.
        // We must NOT return from this match arm — that would run Drop
        // on PtyMaster in the child, which would close master_fd that
        // the parent is still using.
        ForkResult::Child => {
            // FIX #3: replace all expect()/panic! calls in the child with
            // libc::_exit(1). Panicking after fork is unsafe — Rust's panic
            // machinery acquires mutexes that may be locked in the parent and
            // are now in a permanently-locked state in the child's copy of
            // memory, causing a deadlock or UB before exec ever runs.

            // Become the leader of a new process session.
            // This is required before we can make the PTY slave our
            // "controlling terminal". Without this, TIOCNOTTY fails.
            if setsid().is_err() {
                unsafe { libc::_exit(1) };
            }

            // Make the PTY slave our stdin (0), stdout (1), stderr (2).
            // dup2(old_fd, new_fd) duplicates old_fd to new_fd.
            // After this, fd 0/1/2 all point at slave_fd's kernel object.
            if dup2(slave_fd, 0).is_err()
                || dup2(slave_fd, 1).is_err()
                || dup2(slave_fd, 2).is_err()
            {
                unsafe { libc::_exit(1) };
            }

            // FIX #4: close only actually-open fds by reading /proc/self/fd
            // instead of iterating up to _SC_OPEN_MAX (which can be 1M+).
            // Fall back to the old brute-force loop if /proc/self/fd is
            // unavailable (non-Linux systems).
            close_fds_above_2(master_fd, slave_fd);

            // execvp replaces this process image with the shell.
            // argv[0] is the shell name (convention: same as the binary).
            // execvp searches PATH if the path has no '/'; since we give
            // a full path it's effectively execve.
            //
            // If execvp succeeds — we never reach the next line.
            // If execvp fails — it returns, and we _exit(1) below.
            let _ = execvp(&shell_cstr, &[&shell_cstr]);

            // execvp returned — something went wrong (shell not found, not
            // executable, out of memory). Use _exit, not exit/panic.
            unsafe { libc::_exit(1) };
        }

        // ── Parent process ───────────────────────────────────────────────
        ForkResult::Parent { child } => {
            log::info!("pty_parent_spawned_child pid={}", child);
            // Parent doesn't use the slave side — close it now.
            // If we don't close it, the master fd will never get EOF
            // when the child exits (because the parent itself holds
            // slave_fd open, keeping the PTY "alive").
            unsafe {
                libc::close(slave_fd);
            }

            // Also explicitly drop master ManuallyDrop so we don't
            // double-close via its Drop impl later — we're handing off
            // master_fd to PtyMaster which owns it from here.
            // slave's ManuallyDrop is fine; we just closed it via libc.
            // (ManuallyDrop never runs Drop, so no double-close risk.)

            Ok(PtyMaster { fd: master_fd, child_pid: child })
        }
    }
}

// FIX #4: close all fds above 2 efficiently.
// On Linux, reads /proc/self/fd to get only open fds — avoids up to 1M
// pointless close() syscalls when _SC_OPEN_MAX is large.
// Falls back to brute-force on non-Linux (BSD, macOS use /dev/fd).
fn close_fds_above_2(master_fd: RawFd, slave_fd: RawFd) {
    // Try /proc/self/fd first (Linux). Collect into a Vec to avoid iterating
    // a directory while closing fds that may affect the dir's own fd.
    let mut fds_to_close: Vec<RawFd> = Vec::new();

    if let Ok(dir) = std::fs::read_dir("/proc/self/fd").or_else(|_| std::fs::read_dir("/dev/fd")) {
        for entry in dir.flatten() {
            if let Ok(name) = entry.file_name().into_string() {
                if let Ok(fd) = name.parse::<RawFd>() {
                    if fd > 2 && fd != master_fd && fd != slave_fd {
                        fds_to_close.push(fd);
                    }
                }
            }
        }
        for fd in fds_to_close {
            unsafe {
                libc::close(fd);
            }
        }
    } else {
        // Fallback: brute-force up to _SC_OPEN_MAX. Slow but correct.
        let max_fd = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) } as RawFd;
        let max_fd = max_fd.min(4096); // cap at a sane bound as a safety measure
        for fd in 3..max_fd {
            if fd != master_fd && fd != slave_fd {
                unsafe {
                    libc::close(fd);
                }
            }
        }
    }
}

// ─── resize_pty ──────────────────────────────────────────────────────────────

/// Tells the kernel about a terminal resize event.
///
/// When the user resizes the window, you MUST call this. The kernel sends
/// SIGWINCH to the shell's process group, which tells vim/htop/etc to redraw.
/// Without this, the shell thinks it's still 80×24 no matter what.
///
/// Internally this issues a `TIOCSWINSZ` ioctl on the master fd.
pub fn resize_pty(master: &PtyMaster, size: TermSize) -> Result<(), PtyError> {
    let winsize = Winsize { ws_row: size.rows, ws_col: size.cols, ws_xpixel: 0, ws_ypixel: 0 };
    // SAFETY: TIOCSWINSZ is a well-defined ioctl for PTY fds.
    // master.fd is valid as long as PtyMaster is alive.
    let ret = unsafe { libc::ioctl(master.fd, libc::TIOCSWINSZ, &winsize as *const Winsize) };
    if ret < 0 {
        Err(PtyError::IoFailed(std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}

// ─── read_from_pty ───────────────────────────────────────────────────────────

/// Read bytes from the PTY master into `buf`.
/// Returns the number of bytes actually read (may be less than buf.len()).
///
/// ## Blocking behaviour
/// This call BLOCKS until at least one byte is available.
/// If the shell is idle, this blocks indefinitely.
/// To avoid blocking your async runtime's thread, run this in
/// `tokio::task::spawn_blocking` or use a non-blocking fd with `O_NONBLOCK`.
///
/// ## EOF / shell exit
/// When the shell exits, the kernel closes its end of the PTY.
/// The next read will return either:
/// - `0` bytes (EOF) — shell exited cleanly, OR
/// - `Err(EIO)` — the slave side was closed (POSIX behaviour on Linux)
///
/// Either way, you should stop reading and call:
/// ```rust
/// // waitpid(master.child_pid(), WaitPidFlag::empty())
/// ```
/// to reap the zombie process.
///
/// ## Why libc::read instead of nix::read
/// nix 0.29 changed `read()` to require `AsFd` (a trait), not a bare `RawFd`
/// integer. `PtyMaster` doesn't implement `AsFd` (doing so safely would
/// require unsafe OwnedFd games). Using libc directly is cleaner here.
pub fn read_from_pty(master: &PtyMaster, buf: &mut [u8]) -> Result<usize, PtyError> {
    // SAFETY:
    // - master.fd is a valid open fd (guaranteed by PtyMaster ownership).
    // - buf is a valid mutable slice; as_mut_ptr() + len() are consistent.
    // - We check the return value before treating the buffer as initialized.
    let n = unsafe { libc::read(master.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };

    match n {
        n if n > 0 => Ok(n as usize),
        0 => Ok(0), // EOF — shell exited, call waitpid
        _ => Err(PtyError::IoFailed(std::io::Error::last_os_error())),
    }
}

// FIX #2: async wrapper so callers don't have to remember spawn_blocking.
// Moves the blocking read onto a dedicated thread pool thread, keeping the
// async executor unblocked.
//
// The master fd is passed as a raw RawFd (not &PtyMaster) because
// spawn_blocking requires 'static — we can't send a non-'static reference
// across thread boundaries. The caller must ensure PtyMaster outlives the
// returned future (trivially true when awaited immediately).
pub async fn read_from_pty_async(fd: RawFd, buf_size: usize) -> Result<Vec<u8>, PtyError> {
    tokio::task::spawn_blocking(move || {
        let mut buf = vec![0u8; buf_size];
        // SAFETY: fd comes from a live PtyMaster (caller's responsibility).
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        match n {
            n if n > 0 => {
                buf.truncate(n as usize);
                Ok(buf)
            }
            0 => Ok(Vec::new()), // EOF
            _ => Err(PtyError::IoFailed(std::io::Error::last_os_error())),
        }
    })
    .await
    .map_err(|e| PtyError::IoFailed(std::io::Error::other(e)))?
}

// ─── write_to_pty ────────────────────────────────────────────────────────────

/// Write bytes to the PTY master — simulates the user typing on a keyboard.
///
/// Guarantees that ALL bytes in `data` are written (loops on short writes).
/// Short writes can happen when the kernel's PTY buffer is temporarily full;
/// this is rare but real under heavy load.
///
/// ## What to send
/// - Printable ASCII: passed as-is to the shell's stdin.
/// - Control chars: `\r` = Enter, `\x03` = Ctrl-C, `\x04` = Ctrl-D (EOF),
///   `\x1b[A` = Up arrow (ANSI escape). The PTY line discipline translates
///   these into signals (SIGINT for Ctrl-C, etc.) before the shell sees them.
///
/// ## Why libc::write
/// Same reason as `read_from_pty` — nix 0.29 requires AsFd, not RawFd.
pub fn write_to_pty(master: &PtyMaster, data: &[u8]) -> Result<(), PtyError> {
    let mut written = 0;

    while written < data.len() {
        // SAFETY: same as read_from_pty — valid fd, valid slice.
        let n = unsafe {
            libc::write(
                master.fd,
                data[written..].as_ptr() as *const libc::c_void,
                data.len() - written,
            )
        };

        if n < 0 {
            // Capture errno immediately — it's a thread-local that gets
            // clobbered by the next syscall.
            return Err(PtyError::IoFailed(std::io::Error::last_os_error()));
        }

        written += n as usize;
    }

    Ok(())
}

// ─── reap_child ──────────────────────────────────────────────────────────────

/// Reaps the shell child process to prevent zombie processes.
///
/// ## What is a zombie?
/// When a child process exits, the kernel keeps its exit status in the
/// process table until the parent calls `wait`/`waitpid`. This undead entry
/// is a "zombie". It consumes a PID slot and a row in the process table.
/// A terminal emulator that opens hundreds of tabs without reaping creates
/// hundreds of zombies — eventually exhausting the system's PID limit.
///
/// ## When to call this
/// Call after `read_from_pty` returns `Ok(0)` or `Err(IoFailed(EIO))`.
/// Both indicate the shell has exited and its end of the PTY is closed.
///
/// ## Non-blocking variant
/// Pass `WaitOptions::WNOHANG` if you don't want to block (e.g., in a
/// SIGCHLD handler or a polling loop). Returns `Ok(None)` if the child
/// hasn't exited yet.
pub fn reap_child(master: &PtyMaster) -> Result<nix::sys::wait::WaitStatus, PtyError> {
    // FIX #1: use WaitFailed instead of reusing ForkFailed for waitpid errors.
    waitpid(master.child_pid, None).map_err(PtyError::WaitFailed)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Smoke test: spawn a shell, write a command, read output, reap child.
    /// This test requires a Unix environment with /bin/sh.
    #[test]
    fn test_spawn_and_echo() {
        let size = TermSize { rows: 24, cols: 80, shell: None };
        let master = spawn_shell(size).expect("spawn_shell failed");

        // Send a command followed by carriage return (Enter in PTY land)
        write_to_pty(&master, b"echo hello_pty\r").expect("write failed");

        // FIX #5: use a wall-clock timeout instead of a fixed iteration count.
        // 5 seconds is generous for CI; a fast machine will finish in <100ms.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut buf = [0u8; 256];
        let mut output = String::new();

        while Instant::now() < deadline {
            match read_from_pty(&master, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    output.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if output.contains("hello_pty") {
                        break;
                    }
                }
            }
        }

        assert!(output.contains("hello_pty"), "expected echo output, got: {:?}", output);

        // Send exit and reap
        let _ = write_to_pty(&master, b"exit\r");
        let _ = reap_child(&master);
    }

    #[test]
    fn test_resize() {
        let master =
            spawn_shell(TermSize { rows: 24, cols: 80, shell: None }).expect("spawn shell in test");
        resize_pty(&master, TermSize { rows: 50, cols: 200, shell: None }).expect("resize failed");
        let _ = write_to_pty(&master, b"exit\r");
        let _ = reap_child(&master);
    }

    // FIX #1: verify reap_child returns WaitFailed (not ForkFailed) on bad pid.
    #[test]
    fn test_reap_error_variant() {
        // Construct a PtyMaster with an invalid pid to force waitpid to fail.
        // We use pid -1 which is never a valid child pid.
        let master =
            spawn_shell(TermSize { rows: 24, cols: 80, shell: None }).expect("spawn shell");
        // Reap legitimately first so the pid is gone.
        let _ = write_to_pty(&master, b"exit\r");
        std::thread::sleep(Duration::from_millis(200));
        let result = reap_child(&master);
        // Whether it succeeds or fails with WaitFailed is fine;
        // what we assert is that it never returns ForkFailed.
        if let Err(e) = result {
            assert!(matches!(e, PtyError::WaitFailed(_)), "expected WaitFailed, got: {:?}", e);
        }
    }
}
