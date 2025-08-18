use nix::unistd::{fork, ForkResult, execvp, dup2, pipe, close};
use nix::sys::wait::waitpid;
use nix::fcntl::{open, OFlag};
use nix::sys::stat::Mode;
use std::ffi::CString;
use std::io::{self, Write};

fn main() {
    loop {
        print!("vssh> ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        if io::stdin().read_line(&mut input).is_err() {
            continue;
        }
        let input = input.trim();
        if input.is_empty() {
            continue;
        }
        if input == "exit" {
            break;
        }

        // Split on pipes
        let pipeline_parts: Vec<&str> = input.split('|').map(|s| s.trim()).collect();
        if pipeline_parts.len() == 1 {
            // Just a single command
            execute_command(pipeline_parts[0], None, None);
        } else {
            // Multiple commands in a pipeline
            execute_pipeline(pipeline_parts);
        }
    }
}

fn execute_command(cmd: &str, input_fd: Option<i32>, output_fd: Option<i32>) {
    // Handle redirection
    let mut parts: Vec<&str> = cmd.split_whitespace().collect();
    let mut infile: Option<&str> = None;
    let mut outfile: Option<&str> = None;

    let mut i = 0;
    while i < parts.len() {
        match parts[i] {
            "<" if i + 1 < parts.len() => {
                infile = Some(parts[i + 1]);
                parts.drain(i..=i + 1);
            }
            ">" if i + 1 < parts.len() => {
                outfile = Some(parts[i + 1]);
                parts.drain(i..=i + 1);
            }
            _ => i += 1,
        }
    }

    if parts.is_empty() {
        return;
    }

    let argv: Vec<CString> = parts.iter().map(|&s| CString::new(s).unwrap()).collect();

    match unsafe { fork() } {
        Ok(ForkResult::Child) => {
            // Input redirection
            if let Some(file) = infile {
                let fd = open(file, OFlag::O_RDONLY, Mode::empty()).expect("cannot open input");
                dup2(fd, 0).expect("dup2 failed for stdin");
                close(fd).unwrap();
            }
            if let Some(fd) = input_fd {
                dup2(fd, 0).expect("dup2 failed for pipeline input");
                close(fd).unwrap();
            }

            // Output redirection
            if let Some(file) = outfile {
                let fd = open(file, OFlag::O_CREAT | OFlag::O_WRONLY | OFlag::O_TRUNC, Mode::from_bits(0o644).unwrap()).expect("cannot open output");
                dup2(fd, 1).expect("dup2 failed for stdout");
                close(fd).unwrap();
            }
            if let Some(fd) = output_fd {
                dup2(fd, 1).expect("dup2 failed for pipeline output");
                close(fd).unwrap();
            }

            execvp(&argv[0], &argv).expect("exec failed");
        }
        Ok(ForkResult::Parent { child }) => {
            waitpid(child, None).unwrap();
        }
        Err(e) => {
            eprintln!("fork failed: {}", e);
        }
    }
}

fn execute_pipeline(commands: Vec<&str>) {
    let mut fds: Vec<[i32; 2]> = Vec::new();

    for _ in 0..commands.len() - 1 {
        fds.push(pipe().unwrap());
    }

    for (i, cmd) in commands.iter().enumerate() {
        let input_fd = if i == 0 { None } else { Some(fds[i - 1][0]) };
        let output_fd = if i == commands.len() - 1 { None } else { Some(fds[i][1]) };

        match unsafe { fork() } {
            Ok(ForkResult::Child) => {
                if let Some(fd) = input_fd {
                    dup2(fd, 0).unwrap();
                }
                if let Some(fd) = output_fd {
                    dup2(fd, 1).unwrap();
                }

                for pipe in &fds {
                    close(pipe[0]).ok();
                    close(pipe[1]).ok();
                }

                execute_command(cmd, None, None);
                std::process::exit(0);
            }
            Ok(ForkResult::Parent { .. }) => {}
            Err(e) => eprintln!("fork failed: {}", e),
        }
    }

    for pipe in &fds {
        close(pipe[0]).ok();
        close(pipe[1]).ok();
    }

    for _ in 0..commands.len() {
        waitpid(-1, None).unwrap();
    }
}
