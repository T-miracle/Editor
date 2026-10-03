//! Localized import outcomes distinguish complete files, document edits and effects without a receipt.

use plugin_protocol::api;

/// Causes are stable domain data, so host diagnostic wording never determines user-visible behavior.
#[derive(Clone, Copy)]
pub(super) enum Reason {
    SourceChanged,
    Superseded,
    InvalidSelection,
    InvalidInput,
    NamesExhausted,
    Host(api::ErrorCode),
}

/// Confirmed files remain useful even when the corresponding source edit cannot be applied.
pub(super) struct Notice {
    pub saved: Vec<String>,
    pub outcome: Outcome,
}

/// Later failed intents cannot erase confirmed files whose references were never confirmed.
/// Controlled img/img1..img4096 basenames keep this path-local namespace finite; presentation is bounded separately.
#[derive(Default)]
pub(super) struct History {
    latest: Option<Notice>,
    retained: Vec<String>,
}

impl History {
    /// Preserve complete-file facts; only an actual edit receipt confirms this intent's references.
    pub(super) fn record(&mut self, notice: Notice) {
        if matches!(&notice.outcome, Outcome::Inserted) {
            self.retained.retain(|name| !notice.saved.contains(name));
        } else {
            for name in &notice.saved {
                if !self.retained.contains(name) {
                    self.retained.push(name.clone());
                }
            }
        }
        self.latest = Some(notice);
    }

    /// Always show the current batch's full receipt, plus recent earlier names without exceeding UI text quotas.
    pub(super) fn message(&self, english: bool) -> Option<String> {
        let latest = self.latest.as_ref()?;
        let mut message = latest.message(english);
        let older = self
            .retained
            .iter()
            .rev()
            .filter(|name| !latest.saved.contains(name))
            .collect::<Vec<_>>();
        if !older.is_empty() {
            let names = older
                .iter()
                .take(32)
                .map(|name| name.as_str())
                .collect::<Vec<_>>()
                .join(if english { ", " } else { "、" });
            message.push_str(&if english {
                format!(" Previously retained saved images: {names}.")
            } else {
                format!("此前保留的已保存图片：{names}。")
            });
            if older.len() > 32 {
                message.push_str(&if english {
                    format!(" {} more saved images retained.", older.len() - 32)
                } else {
                    format!("另有 {} 张已保存图片保留。", older.len() - 32)
                });
            }
        }
        Some(message)
    }
}

/// Cancellation after a side effect starts cannot prove whether the file or reference was written.
pub(super) enum Outcome {
    Inserted,
    NotInserted(Reason),
    UnconfirmedFile { name: String, reason: Reason },
    UnconfirmedReferences(Reason),
}

impl Notice {
    /// Names are validated basenames from this intent's receipts, never host paths or clipboard bytes.
    pub(super) fn message(&self, english: bool) -> String {
        let names = self.saved.join(if english { ", " } else { "、" });
        match &self.outcome {
            Outcome::Inserted => {
                if english {
                    format!(
                        "Saved images and inserted references: {names}. Undo keeps the image files."
                    )
                } else {
                    format!("图片已保存并插入引用：{names}。撤销引用会保留图片文件。")
                }
            }
            outcome => {
                let mut message = if self.saved.is_empty() {
                    if english {
                        "No image save was confirmed for this import.".into()
                    } else {
                        "本次没有已确认保存的图片。".into()
                    }
                } else if english {
                    format!("Saved images retained: {names}.")
                } else {
                    format!("已保存并保留图片：{names}。")
                };
                let reason = match outcome {
                    Outcome::NotInserted(reason) => {
                        message.push_str(if english {
                            " References were not inserted. "
                        } else {
                            "引用未插入。"
                        });
                        *reason
                    }
                    Outcome::UnconfirmedFile { name, reason } => {
                        message.push_str(&if english {
                            format!(" Save result for {name} is unconfirmed; references were not inserted. ")
                        } else {
                            format!("{name} 的保存结果未确认；引用未插入。")
                        });
                        *reason
                    }
                    Outcome::UnconfirmedReferences(reason) => {
                        message.push_str(if english {
                            " Reference insertion is unconfirmed. "
                        } else {
                            "引用插入结果未确认。"
                        });
                        *reason
                    }
                    Outcome::Inserted => unreachable!("handled above"),
                };
                message.push_str(reason.message(english));
                message
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{History, Notice, Outcome, Reason};
    use plugin_protocol::api;

    /// A failed later paste must still explain the earlier complete files after reopening the source path.
    #[test]
    fn a_new_failed_import_does_not_hide_saved_files_without_confirmed_references() {
        let mut history = History::default();
        history.record(Notice {
            saved: vec!["img.png".into(), "img1.jpg".into()],
            outcome: Outcome::NotInserted(Reason::SourceChanged),
        });
        history.record(Notice {
            saved: Vec::new(),
            outcome: Outcome::NotInserted(Reason::Host(api::ErrorCode::PermissionDenied)),
        });
        for english in [false, true] {
            let message = history.message(english).unwrap();
            assert!(
                message.contains("img.png") && message.contains("img1.jpg"),
                "retained complete-file receipt: {message}"
            );
            assert!(message.contains(if english {
                "References were not inserted"
            } else {
                "引用未插入"
            }));
            assert!(message.contains(if english { "permission" } else { "权限" }));
        }
        history.record(Notice {
            saved: vec!["img2.webp".into()],
            outcome: Outcome::Inserted,
        });
        let message = history.message(true).unwrap();
        assert!(
            message.contains("img2.webp")
                && message.contains("img.png")
                && message.contains("img1.jpg")
        );
        assert!(message.contains("Undo keeps the image files"));
    }
}

impl Reason {
    /// File-write failure, selection conflict and source invalidation each explain a distinct recovery.
    fn message(self, english: bool) -> &'static str {
        let (chinese, english_message) = match self {
            Self::SourceChanged | Self::Host(api::ErrorCode::StaleRevision) => (
                "文档已改变或关闭，请确认文件后重新插入引用。",
                "The document changed or closed. Check the files before inserting references again.",
            ),
            Self::Superseded => (
                "有新的编辑操作，本次图片导入已停止。",
                "A newer edit stopped this image import.",
            ),
            Self::InvalidSelection => (
                "当前选区不能生成有效图片引用，请调整选区。",
                "The current selection cannot form valid image references. Please adjust the selection.",
            ),
            Self::InvalidInput | Self::Host(api::ErrorCode::InvalidRequest) => (
                "图片输入或保存收据无效，未继续导入。",
                "The image input or save receipt is invalid. Import did not continue.",
            ),
            Self::NamesExhausted => (
                "图片编号已达到 img4096，未继续保存。",
                "Image numbering reached img4096. No further saves were started.",
            ),
            Self::Host(api::ErrorCode::PermissionDenied) => (
                "没有图片输入或文件写入权限。",
                "Image input or file-write permission is missing.",
            ),
            Self::Host(api::ErrorCode::Conflict) => (
                "选区已改变，请重新插入引用。",
                "The selection changed. Please insert the references again.",
            ),
            Self::Host(api::ErrorCode::InvalidPath) => (
                "图片目标目录不可写或路径不被允许。",
                "The image directory is not writable or the path is not allowed.",
            ),
            Self::Host(
                api::ErrorCode::InvalidHandle
                | api::ErrorCode::NotFound
                | api::ErrorCode::InvalidState,
            ) => (
                "源文档或图片输入已关闭、过期或不可用。",
                "The source document or image input is closed, expired or unavailable.",
            ),
            Self::Host(api::ErrorCode::TimedOut) => ("图片导入超时。", "Image import timed out."),
            Self::Host(api::ErrorCode::Cancelled) => {
                ("图片导入已取消。", "Image import was cancelled.")
            }
            Self::Host(api::ErrorCode::LimitExceeded) => (
                "图片导入超过数量、大小或请求限制。",
                "Image import exceeds the count, size or request limits.",
            ),
            Self::Host(
                api::ErrorCode::UnsupportedOperation | api::ErrorCode::CapabilityUnavailable,
            ) => (
                "当前宿主不支持所需的图片导入能力。",
                "The host does not support the required image-import capability.",
            ),
            Self::Host(api::ErrorCode::OperationFailed) => (
                "图片文件保存或引用编辑失败，请检查文件与目录。",
                "Image saving or reference editing failed. Please check the files and directory.",
            ),
        };
        if english { english_message } else { chinese }
    }
}
