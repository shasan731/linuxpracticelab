use std::path::Path;

/// Renders a host path in the form accepted by QEMU on Windows.
///
/// `std::fs::canonicalize` may add the Windows extended-length prefix (`\\?\`).
/// QEMU does not understand that prefix after slash normalisation (`//?/C:/...`) and
/// aborts while opening the disk. UNC paths need their own equivalent conversion.
pub(crate) fn render(path: &Path) -> String {
    let rendered = path.to_string_lossy().replace('\\', "/");
    if let Some(path) = rendered.strip_prefix("//?/UNC/") {
        format!("//{path}")
    } else if let Some(path) = rendered.strip_prefix("//?/") {
        path.to_string()
    } else {
        rendered
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_the_windows_extended_length_prefix() {
        assert_eq!(
            render(Path::new(r"\\?\C:\LinuxLab\runtime\vmlinuz")),
            "C:/LinuxLab/runtime/vmlinuz"
        );
    }

    #[test]
    fn preserves_unc_semantics_when_removing_the_extended_prefix() {
        assert_eq!(
            render(Path::new(r"\\?\UNC\server\share\debian-base.raw")),
            "//server/share/debian-base.raw"
        );
    }

    #[test]
    fn ordinary_windows_paths_are_slash_normalised() {
        assert_eq!(
            render(Path::new(r"F:\LinuxLab\data\practice.qcow2")),
            "F:/LinuxLab/data/practice.qcow2"
        );
    }
}
