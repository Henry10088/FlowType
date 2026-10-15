use super::FileTab;
use super::ui_pages::{file_failed_summary, file_transfer_status};
use crate::file_transfer_server::FileTransferSnapshot;
use crate::i18n::tr;
use flowtype_core::file_transfer::TransferDirection;

// Both initial page creation and incremental updates use this projection.
// Cancellation belongs to the displayed batch, not whichever batch starts next.
pub(super) struct FileTabContent {
    pub status: String,
    pub failures: String,
    pub can_choose: bool,
    pub cancel_batch: Option<String>,
}

impl FileTabContent {
    pub(super) fn new(tab: FileTab, transfer: Option<&FileTransferSnapshot>) -> Self {
        let direction = match tab {
            FileTab::Send => TransferDirection::ComputerToPhone,
            FileTab::Received => TransferDirection::PhoneToComputer,
        };
        let visible = transfer.filter(|value| value.direction == direction);
        let busy = transfer.is_some_and(FileTransferSnapshot::active);
        let status = visible.map_or_else(
            || {
                match (tab, busy) {
                    (FileTab::Send, true) => tr(
                        "正在接收手机文件，请在「收到的文件」查看或取消",
                        "Receiving files. View or cancel in Received files.",
                    ),
                    (FileTab::Received, true) => tr(
                        "正在发送文件，请在「发送到手机」查看或取消",
                        "Sending files. View or cancel in Send to phone.",
                    ),
                    (FileTab::Send, false) => tr(
                        "选择文件后，手机只需确认一次",
                        "The phone confirms once per transfer",
                    ),
                    (FileTab::Received, false) => {
                        tr("等待手机发送文件", "Waiting for files from your phone")
                    }
                }
                .to_owned()
            },
            file_transfer_status,
        );
        Self {
            status,
            failures: file_failed_summary(visible),
            can_choose: tab == FileTab::Send && !busy,
            cancel_batch: visible
                .filter(|value| value.active())
                .map(|value| value.batch_id.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowtype_core::file_transfer::BatchState;

    fn batch(direction: TransferDirection, state: BatchState) -> FileTransferSnapshot {
        FileTransferSnapshot {
            batch_id: "visible-batch".into(),
            direction,
            state,
            completed_bytes: 4096,
            total_bytes: 8192,
            completed_entries: 1,
            failed_entries: 1,
            failed_paths: vec!["failed-document.pdf".into()],
            total_entries: 3,
        }
    }

    #[test]
    fn only_the_matching_tab_shows_progress_failures_and_cancellation() {
        for (direction, own_tab, other_tab) in [
            (
                TransferDirection::PhoneToComputer,
                FileTab::Received,
                FileTab::Send,
            ),
            (
                TransferDirection::ComputerToPhone,
                FileTab::Send,
                FileTab::Received,
            ),
        ] {
            let transfer = batch(direction, BatchState::Transferring);
            let own = FileTabContent::new(own_tab, Some(&transfer));
            assert_eq!(own.status, file_transfer_status(&transfer));
            assert!(own.failures.contains("failed-document.pdf"));
            assert_eq!(own.cancel_batch.as_deref(), Some("visible-batch"));
            assert!(!own.can_choose);

            let other = FileTabContent::new(other_tab, Some(&transfer));
            assert_ne!(other.status, own.status);
            assert!(!other.status.contains("KB"));
            assert!(other.failures.is_empty());
            assert!(other.cancel_batch.is_none());
            assert!(!other.can_choose);
        }
    }

    #[test]
    fn terminal_results_stay_in_their_direction_without_cancel_actions() {
        for state in [
            BatchState::Completed,
            BatchState::Canceled,
            BatchState::PartialFailure,
        ] {
            let transfer = batch(TransferDirection::PhoneToComputer, state);
            let received = FileTabContent::new(FileTab::Received, Some(&transfer));
            assert_eq!(received.status, file_transfer_status(&transfer));
            assert!(received.cancel_batch.is_none());

            let send = FileTabContent::new(FileTab::Send, Some(&transfer));
            assert_eq!(send.status, FileTabContent::new(FileTab::Send, None).status);
            assert!(send.failures.is_empty());
            assert!(send.can_choose);
        }
    }

    #[test]
    fn progress_updates_and_new_batches_refresh_the_cancel_target() {
        let mut transfer = batch(
            TransferDirection::PhoneToComputer,
            BatchState::WaitingNetwork,
        );
        let waiting = FileTabContent::new(FileTab::Received, Some(&transfer));
        assert_eq!(waiting.cancel_batch.as_deref(), Some("visible-batch"));
        transfer.completed_bytes = 6144;
        transfer.state = BatchState::Transferring;
        let resumed = FileTabContent::new(FileTab::Received, Some(&transfer));
        assert_ne!(resumed.status, waiting.status);
        assert_eq!(resumed.cancel_batch, waiting.cancel_batch);
        transfer.batch_id = "next-batch".into();
        assert_eq!(
            FileTabContent::new(FileTab::Received, Some(&transfer))
                .cancel_batch
                .as_deref(),
            Some("next-batch")
        );
        let disconnected = FileTabContent::new(FileTab::Received, None);
        assert!(disconnected.cancel_batch.is_none());
        assert!(disconnected.failures.is_empty());
    }
}
