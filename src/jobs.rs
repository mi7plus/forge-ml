use std::collections::VecDeque;
use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum JobState {
    Queued,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone)]
pub struct TrainingJob {
    pub id: usize,
    pub command: String,
    pub state: JobState,
    pub queued_at: Instant,
    pub started_at: Option<Instant>,
    pub elapsed: Duration,
    pub output: String,
}

struct Work {
    id: usize,
    command: String,
    root: PathBuf,
}
struct Finished {
    id: usize,
    success: bool,
    output: String,
    elapsed: Duration,
}

pub struct JobQueue {
    pub jobs: VecDeque<TrainingJob>,
    sender: Sender<Work>,
    receiver: Receiver<Finished>,
    next_id: usize,
    active: Option<usize>,
}

impl JobQueue {
    pub fn new() -> Self {
        let (work_tx, work_rx) = mpsc::channel::<Work>();
        let (result_tx, result_rx) = mpsc::channel();
        thread::spawn(move || {
            while let Ok(work) = work_rx.recv() {
                let started = Instant::now();
                let output = if cfg!(windows) {
                    Command::new("cmd")
                        .args(["/C", &work.command])
                        .current_dir(&work.root)
                        .output()
                } else {
                    Command::new("sh")
                        .args(["-lc", &work.command])
                        .current_dir(&work.root)
                        .output()
                };
                let finished = match output {
                    Ok(output) => Finished {
                        id: work.id,
                        success: output.status.success(),
                        output: format!(
                            "{}{}",
                            String::from_utf8_lossy(&output.stdout),
                            String::from_utf8_lossy(&output.stderr)
                        ),
                        elapsed: started.elapsed(),
                    },
                    Err(error) => Finished {
                        id: work.id,
                        success: false,
                        output: error.to_string(),
                        elapsed: started.elapsed(),
                    },
                };
                let _ = result_tx.send(finished);
            }
        });
        Self {
            jobs: VecDeque::new(),
            sender: work_tx,
            receiver: result_rx,
            next_id: 1,
            active: None,
        }
    }
    pub fn enqueue(&mut self, command: String, root: PathBuf) -> Result<usize, String> {
        if command.trim().is_empty() {
            return Err("Enter a training command first.".into());
        }
        let id = self.next_id;
        self.next_id += 1;
        self.jobs.push_back(TrainingJob {
            id,
            command: command.clone(),
            state: JobState::Queued,
            queued_at: Instant::now(),
            started_at: None,
            elapsed: Duration::ZERO,
            output: String::new(),
        });
        if self.active.is_none() {
            self.start(id, command, root)?;
        }
        Ok(id)
    }
    fn start(&mut self, id: usize, command: String, root: PathBuf) -> Result<(), String> {
        self.sender
            .send(Work { id, command, root })
            .map_err(|e| e.to_string())?;
        self.active = Some(id);
        if let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) {
            job.state = JobState::Running;
            job.started_at = Some(Instant::now());
        }
        Ok(())
    }
    pub fn poll(&mut self, root: PathBuf) {
        while let Ok(result) = self.receiver.try_recv() {
            if let Some(job) = self.jobs.iter_mut().find(|job| job.id == result.id) {
                job.state = if result.success {
                    JobState::Completed
                } else {
                    JobState::Failed
                };
                job.output = result.output;
                job.elapsed = result.elapsed;
            }
            self.active = None;
            if let Some(job) = self.jobs.iter().find(|job| job.state == JobState::Queued) {
                let _ = self.start(job.id, job.command.clone(), root.clone());
            }
        }
        if let Some(id) = self.active {
            if let Some(job) = self.jobs.iter_mut().find(|job| job.id == id) {
                job.elapsed = job
                    .started_at
                    .map(|started| started.elapsed())
                    .unwrap_or_default();
            }
        }
    }
    pub fn eta(&self) -> Option<Duration> {
        let completed = self
            .jobs
            .iter()
            .filter(|job| job.state == JobState::Completed)
            .collect::<Vec<_>>();
        if completed.is_empty() {
            return None;
        }
        let average = completed
            .iter()
            .map(|job| job.elapsed.as_secs_f64())
            .sum::<f64>()
            / completed.len() as f64;
        let remaining = self
            .jobs
            .iter()
            .filter(|job| matches!(job.state, JobState::Queued | JobState::Running))
            .count();
        Some(Duration::from_secs_f64(average * remaining as f64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_empty_jobs() {
        let mut queue = JobQueue::new();
        assert!(queue.enqueue("".into(), PathBuf::from(".")).is_err());
    }
}
