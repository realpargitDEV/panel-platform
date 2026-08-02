//! Volumes, their capacity and their kind.

use crate::snapshot::{StorageKind, VolumeInfo};

pub(crate) fn read_volumes(disks: &sysinfo::Disks) -> Vec<VolumeInfo> {
    disks
        .list()
        .iter()
        .map(|disk| VolumeInfo {
            mount_point: disk.mount_point().display().to_string(),
            total_bytes: disk.total_space(),
            free_bytes: disk.available_space(),
            removable: disk.is_removable(),
            kind: match disk.kind() {
                sysinfo::DiskKind::SSD => StorageKind::Ssd,
                sysinfo::DiskKind::HDD => StorageKind::Hdd,
                // Reported rather than guessed. Storage kind informs nothing
                // that must be decided, so an unknown one is not worth a probe
                // that could be wrong.
                _ => StorageKind::Unknown,
            },
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::SystemSnapshot;

    #[test]
    fn this_machine_reports_at_least_one_volume() {
        let disks = sysinfo::Disks::new_with_refreshed_list();
        let volumes = read_volumes(&disks);
        assert!(!volumes.is_empty(), "a running machine has a filesystem");
        for volume in &volumes {
            assert!(
                volume.free_bytes <= volume.total_bytes,
                "{} reports more free than total",
                volume.mount_point
            );
        }
    }

    #[test]
    fn removable_volumes_are_excluded_from_capacity() {
        // A USB stick with 400 GB free must not make a machine look roomy.
        let mut snapshot = SystemSnapshot::unknown();
        snapshot.volumes = vec![
            VolumeInfo {
                mount_point: "C:\\".to_string(),
                total_bytes: 250_000_000_000,
                free_bytes: 10_000_000_000,
                removable: false,
                kind: StorageKind::Ssd,
            },
            VolumeInfo {
                mount_point: "E:\\".to_string(),
                total_bytes: 500_000_000_000,
                free_bytes: 400_000_000_000,
                removable: true,
                kind: StorageKind::Unknown,
            },
        ];
        assert_eq!(
            snapshot.largest_fixed_free_bytes(),
            Some(10_000_000_000),
            "the removable volume must not count"
        );
    }

    #[test]
    fn a_machine_with_no_volumes_reports_unknown_capacity() {
        assert_eq!(SystemSnapshot::unknown().largest_fixed_free_bytes(), None);
    }
}
