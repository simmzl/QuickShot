//! Serial PNG writes keep capture order without waiting for disk on the event loop.
use crate::config::SaveConfig;
use crate::file_save::{save_png_at, CaptureMode};
use crate::notification::CaptureNotice;
use image::RgbaImage;
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};
use time::OffsetDateTime;

const WAITING_FRAMES: usize = 3;

pub struct Capture {
    pub frame: RgbaImage,
    pub save: SaveConfig,
    pub captured_at: OffsetDateTime,
    pub notify: bool,
    pub copied: bool,
}

pub struct SaveQueue {
    sender: Option<SyncSender<Capture>>,
    worker: Option<JoinHandle<()>>,
}

impl SaveQueue {
    pub fn new(report: impl Fn(Option<CaptureNotice>) + Send + 'static) -> Self {
        let (sender, receiver) = mpsc::sync_channel::<Capture>(WAITING_FRAMES);
        let worker = thread::spawn(move || {
            for capture in receiver {
                let (width, height) = capture.frame.dimensions();
                let saved = match save_png_at(
                    &capture.frame,
                    &capture.save.directory,
                    &capture.save.filename_template,
                    CaptureMode::Fullscreen,
                    capture.captured_at,
                ) {
                    Ok(path) => {
                        println!("saved → {}", path.display());
                        Some(path)
                    }
                    Err(error) => {
                        eprintln!("fullscreen save: {error:#}");
                        None
                    }
                };
                let notice = (capture.notify && (capture.copied || saved.is_some()))
                    .then(|| CaptureNotice::new(width, height, saved));
                report(notice);
            }
        });
        Self {
            sender: Some(sender),
            worker: Some(worker),
        }
    }

    pub fn submit(&self, capture: Capture) -> Result<(), &'static str> {
        self.sender
            .as_ref()
            .unwrap()
            .try_send(capture)
            .map_err(|error| match error {
                TrySendError::Full(_) => "PNG save queue is full",
                TrySendError::Disconnected(_) => "PNG save worker stopped",
            })
    }
}

impl Drop for SaveQueue {
    fn drop(&mut self) {
        drop(self.sender.take());
        if self.worker.take().unwrap().join().is_err() {
            eprintln!("fullscreen save worker stopped unexpectedly");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use time::macros::datetime;

    #[test]
    fn notification_follows_save_and_clipboard_outcomes() {
        let dir = tempfile::tempdir().unwrap();
        let invalid_dir = dir.path().join("file");
        std::fs::write(&invalid_dir, []).unwrap();
        let (send, receive) = mpsc::channel();
        let queue = SaveQueue::new(move |notice| send.send(notice).unwrap());
        for (copied, can_save, notify) in [
            (false, true, true),
            (true, false, true),
            (false, false, true),
            (true, true, false),
        ] {
            queue
                .submit(Capture {
                    frame: RgbaImage::new(1, 1),
                    save: SaveConfig {
                        enabled: true,
                        directory: if can_save {
                            dir.path().into()
                        } else {
                            invalid_dir.clone()
                        },
                        filename_template: "shot.png".into(),
                    },
                    captured_at: datetime!(2026-01-02 03:04:05 UTC),
                    notify,
                    copied,
                })
                .unwrap();
            let notice = receive.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(notice.is_some(), notify && (copied || can_save));
            if let Some(notice) = notice {
                assert_eq!(
                    notice.saved.as_deref().is_some_and(|p| p.is_file()),
                    can_save
                );
            }
        }
    }

    #[test]
    fn slow_writer_bounds_backlog_and_drains_accepted_frames_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let (started, first_done) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let (results, saved) = mpsc::channel();
        let queue = SaveQueue::new(move |notice| {
            let path = notice.unwrap().saved.unwrap();
            if path.file_name().unwrap() == "2026-01-02_03-04-05-1.png" {
                started.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(5)).unwrap();
            }
            results.send(path).unwrap();
        });
        let shot = |width| Capture {
            frame: RgbaImage::new(width, 1),
            save: SaveConfig {
                enabled: true,
                directory: dir.path().into(),
                filename_template: "{datetime}-{w}.png".into(),
            },
            captured_at: datetime!(2026-01-02 03:04:05 UTC),
            notify: true,
            copied: true,
        };
        queue.submit(shot(1)).unwrap();
        first_done.recv_timeout(Duration::from_secs(5)).unwrap();
        for width in 2..=WAITING_FRAMES + 1 {
            queue.submit(shot(width as u32)).unwrap();
        }
        assert!(queue.submit(shot(9)).is_err());
        release.send(()).unwrap();
        drop(queue);
        let paths: Vec<_> = saved.into_iter().collect();
        assert_eq!(paths.len(), WAITING_FRAMES + 1);
        for (i, path) in paths.iter().enumerate() {
            let width = i + 1;
            assert_eq!(
                *path,
                dir.path().join(format!("2026-01-02_03-04-05-{width}.png"))
            );
            assert_eq!(image::image_dimensions(path).unwrap(), (width as u32, 1));
        }
    }
}
