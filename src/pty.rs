// File: src/pty.rs
// Milestone 1: PTY Creation and Shell Process Management
//
// A PTY (Pseudo Terminal) is a pair of virtual devices:
//   - Master: our program reads/writes here
//   - Slave:  the shell process thinks this is a real terminal
//
// This is the foundation of the entire emulator.
// Without this, we have no shell to talk to.

use std::os::unix::io::{RawFd, AsRawFd};
use nix::pty::{openpty, Winsize};
use nix::unistd::{fork, ForkResult, setsid, dup2, execvp};
use std::ffi::CString;
use thiserror::Error;

/// All errors that can occur in PTY operations.
#[derive(Error, Debug)]
pub enum PtyError {
    #[error("Failed to open PTY pair: {0}")]
    OpenFailed(#[from] nix::Error),

    #[error("Fork failed")]
    ForkFailed,

    #[error("I/O error on PTY fd")]
    IoFailed,
}

/// Owns the PTY master file descriptor.
/// Drop closes it automatically — RAII.
pub struct PtyMaster {
    pub fd: RawFd,
}

impl Drop for PtyMaster {
    fn drop(&mut self) {
        // Use libc directly to avoid nix OwnedFd conflicts
        unsafe { libc::close(self.fd); }
    }
}

// Safety: PtyMaster is just a file descriptor integer; sending it
// across threads is safe as long as only one thread uses it at a time,
// which our Arc<PtyMaster> + tokio::select! design guarantees.
unsafe impl Send for PtyMaster {}
unsafe impl Sync for PtyMaster {}

/// Terminal window size in characters.
pub struct TermSize {
    pub rows: u16,
    pub cols: u16,
}

/// Opens a PTY pair and spawns a shell.
/// Returns the master fd — our side of the connection.
pub fn spawn_shell(size: TermSize) -> Result<PtyMaster, PtyError> {
    let winsize = Winsize {
        ws_row: size.rows,
        ws_col: size.cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };

    // openpty returns OwnedFd in nix 0.29 — extract raw fds immediately
    // so OwnedFd drops and we manage lifetime manually via PtyMaster.
    let pty = openpty(Some(&winsize), None)?;
    let master_fd: RawFd = pty.master.as_raw_fd();
    let slave_fd: RawFd = pty.slave.as_raw_fd();

    // Prevent OwnedFd from closing these — we own them now.
    std::mem::forget(pty.master);
    std::mem::forget(pty.slave);

    match unsafe { fork()? } {
        ForkResult::Child => {
            // Child process — become the shell

            setsid().expect("setsid failed");

            // Wire child's stdio to PTY slave
            dup2(slave_fd, 0).expect("dup2 stdin failed");
            dup2(slave_fd, 1).expect("dup2 stdout failed");
            dup2(slave_fd, 2).expect("dup2 stderr failed");

            // Close raw fds in child — already duplicated to 0, 1, 2
            unsafe {
                libc::close(slave_fd);
                libc::close(master_fd);
            }

            // Replace child with the shell — execvp does not return on success;
            // .expect() panics on failure, which is the only way we get here.
            let shell = CString::new("/bin/bash").unwrap();
            let _ = execvp(&shell, &[&shell]);
            std::process::exit(1); // execvp failed — exit child cleanly
        }
        ForkResult::Parent { child: _ } => {
            // Parent process — close slave, keep master
            unsafe { libc::close(slave_fd); }

            Ok(PtyMaster { fd: master_fd })
        }
    }
}

/// Read raw bytes from the PTY master.
/// Uses libc::read directly — nix 0.29's read() requires AsFd, not RawFd.
pub fn read_from_pty(master: &PtyMaster, buf: &mut [u8]) -> Result<usize, PtyError> {
    let n = unsafe {
        libc::read(master.fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len())
    };
    if n < 0 {
        Err(PtyError::IoFailed)
    } else {
        Ok(n as usize)
    }
}

/// Write bytes to the PTY master (simulates keyboard input).
/// Uses libc::write directly — nix 0.29's write() requires AsFd, not RawFd.
pub fn write_to_pty(master: &PtyMaster, data: &[u8]) -> Result<(), PtyError> {
    let mut written = 0;
    while written < data.len() {
        let n = unsafe {
            libc::write(
                master.fd,
                data[written..].as_ptr() as *const libc::c_void,
                data.len() - written,
            )
        };
        if n < 0 {
            return Err(PtyError::IoFailed);
        }
        written += n as usize;
    }
    Ok(())
}