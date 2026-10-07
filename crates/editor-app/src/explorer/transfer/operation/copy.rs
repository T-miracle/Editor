//! Stream file data and prepare same-volume publications without blocking the UI.
use super::*;
use std::io::{Read, Write};

impl Worker {
    /// Write into a temporary sibling and publish only after a complete, verified copy.
    pub(super) async fn file(
        &mut self,
        source: &Path,
        target: &Path,
        original: &Stamp,
    ) -> Result<(), String> {
        self.check_cancel()?;
        let source_stamp = snapshot::stamp(source)?;
        let before = snapshot::capture(target, self.receipt.backup.path())?;
        let source_before = if self.kind == Kind::Move {
            Some(snapshot::capture(source, self.receipt.backup.path())?)
        } else {
            None
        };
        let mut input =
            fs::File::open(source).map_err(|error| format!("{}: {error}", source.display()))?;
        let mut temporary = tempfile::Builder::new()
            .prefix(".me-transfer-")
            .tempfile_in(target.parent().unwrap())
            .map_err(|error| error.to_string())?;
        let mut buffer = vec![0; 256 * 1024];
        loop {
            self.check_cancel()?;
            let count = input
                .read(&mut buffer)
                .map_err(|error| format!("{}: {error}", source.display()))?;
            if count == 0 {
                break;
            }
            temporary
                .write_all(&buffer[..count])
                .map_err(|error| format!("{}: {error}", target.display()))?;
            self.bytes += count as u64;
            if self.bytes % (4 * 1024 * 1024) < count as u64 {
                let _ = self.events.unbounded_send(Event::Progress {
                    path: source.to_path_buf(),
                    completed: self.completed,
                    bytes: self.bytes,
                });
            }
        }
        temporary.flush().map_err(|error| error.to_string())?;
        temporary
            .as_file()
            .sync_all()
            .map_err(|error| error.to_string())?;
        temporary
            .as_file()
            .set_permissions(
                fs::metadata(source)
                    .map_err(|error| error.to_string())?
                    .permissions(),
            )
            .map_err(|error| error.to_string())?;
        self.check_cancel()?;
        snapshot::check_target(&self.root, target)?;
        let mut entries = Vec::new();
        if self.kind == Kind::Move {
            entries.push(Entry::remove(
                source,
                source_before.as_ref().unwrap().clone(),
            )?);
        }
        entries.push(Entry::file(target, temporary, before.clone())?);
        if snapshot::stamp(source)? != source_stamp || snapshot::stamp(target)? != *original {
            return Err(format!("{}: {}", target.display(), t!("transfer.changed")));
        }
        // Final dirty protection and publication run in one UI callback; no expensive hashes follow authorization.
        self.publish(Plan {
            entries,
            root: Some(self.root.clone()),
            movement: (self.kind == Kind::Move)
                .then(|| (source.to_path_buf(), target.to_path_buf())),
            protection: Protection::Ordinary {
                targets: vec![target.to_path_buf()],
            },
        })
        .await?;
        let after = self.expected.stamp(target)?;
        self.receipt.changes.push(Change {
            path: target.to_path_buf(),
            before,
            after,
        });
        if let Some(before) = source_before {
            self.receipt.changes.push(Change {
                path: source.to_path_buf(),
                before,
                after: Stamp::Missing,
            });
            self.receipt
                .moves
                .push((source.to_path_buf(), target.to_path_buf()));
            for file in &mut self.watched {
                if file.path == source {
                    file.path = target.to_path_buf();
                }
            }
        }
        self.completed += 1;
        let _ = self.events.unbounded_send(Event::Progress {
            path: target.to_path_buf(),
            completed: self.completed,
            bytes: self.bytes,
        });
        Ok(())
    }
}
