use std::{
    io::{BufRead as _, BufReader},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use zbus::{
    MatchRule, Message,
    blocking::{Connection, MessageIterator},
    message::Type,
};

const INITIAL_RETRY: Duration = Duration::from_secs(1);
const MAX_RETRY: Duration = Duration::from_secs(30);
const HEALTHY_MONITOR_UPTIME: Duration = Duration::from_secs(30);

#[derive(Clone, Copy)]
pub(crate) enum Bus {
    System,
}

pub(crate) fn start_dbus_signal_monitor<T>(
    thread_name: &'static str,
    bus: Bus,
    sender: &'static str,
    commands: mpsc::Sender<T>,
    refresh: T,
    relevant: fn(&Message) -> bool,
) where
    T: Clone + Send + 'static,
{
    thread::Builder::new()
        .name(thread_name.into())
        .spawn(move || {
            let mut retry = RetryBackoff::default();
            loop {
                let connection = match bus {
                    Bus::System => Connection::system(),
                };
                let connection = match connection {
                    Ok(connection) => connection,
                    Err(error) => {
                        tracing::warn!(%error, %sender, "could not connect D-Bus event monitor");
                        retry.sleep_and_advance();
                        continue;
                    }
                };
                let rule = match MatchRule::builder()
                    .msg_type(Type::Signal)
                    .sender(sender)
                    .map(|builder| builder.build())
                {
                    Ok(rule) => rule,
                    Err(error) => {
                        tracing::warn!(%error, %sender, "could not build D-Bus event rule");
                        return;
                    }
                };
                let mut messages =
                    match MessageIterator::for_match_rule(rule, &connection, Some(64)) {
                        Ok(messages) => messages,
                        Err(error) => {
                            tracing::warn!(%error, %sender, "could not subscribe to D-Bus events");
                            retry.sleep_and_advance();
                            continue;
                        }
                    };
                if commands.send(refresh.clone()).is_err() {
                    return;
                }
                retry.reset();
                loop {
                    match messages.next() {
                        Some(Ok(message)) if relevant(&message) => {
                            if commands.send(refresh.clone()).is_err() {
                                return;
                            }
                        }
                        Some(Ok(_)) => {}
                        Some(Err(error)) => {
                            tracing::warn!(%error, %sender, "D-Bus event monitor disconnected");
                            break;
                        }
                        None => break,
                    }
                }
                retry.sleep_and_advance();
            }
        })
        .expect("failed to start D-Bus event monitor");
}

pub(crate) fn start_process_line_monitor<T>(
    thread_name: &'static str,
    program: &'static str,
    arguments: &'static [&'static str],
    commands: mpsc::Sender<T>,
    refresh: T,
    relevant: fn(&str) -> bool,
) where
    T: Clone + Send + 'static,
{
    thread::Builder::new()
        .name(thread_name.into())
        .spawn(move || {
            let mut retry = RetryBackoff::default();
            loop {
                let mut child = match Command::new(program)
                    .args(arguments)
                    .env("LC_ALL", "C")
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .spawn()
                {
                    Ok(child) => child,
                    Err(error) => {
                        tracing::warn!(%error, %program, "could not start event monitor");
                        retry.sleep_and_advance();
                        continue;
                    }
                };
                let started = Instant::now();
                if commands.send(refresh.clone()).is_err() {
                    let _ = child.kill();
                    return;
                }
                if let Some(stdout) = child.stdout.take() {
                    for line in BufReader::new(stdout).lines() {
                        match line {
                            Ok(line) if relevant(&line) => {
                                if commands.send(refresh.clone()).is_err() {
                                    let _ = child.kill();
                                    return;
                                }
                            }
                            Ok(_) => {}
                            Err(error) => {
                                tracing::warn!(%error, %program, "event monitor output failed");
                                break;
                            }
                        }
                    }
                }
                let _ = child.wait();
                if started.elapsed() >= HEALTHY_MONITOR_UPTIME {
                    retry.reset();
                }
                retry.sleep_and_advance();
            }
        })
        .expect("failed to start process event monitor");
}

struct RetryBackoff {
    next: Duration,
}

impl Default for RetryBackoff {
    fn default() -> Self {
        Self {
            next: INITIAL_RETRY,
        }
    }
}

impl RetryBackoff {
    fn reset(&mut self) {
        self.next = INITIAL_RETRY;
    }

    fn sleep_and_advance(&mut self) {
        thread::sleep(self.next);
        self.next = self.next.saturating_mul(2).min(MAX_RETRY);
    }
}
