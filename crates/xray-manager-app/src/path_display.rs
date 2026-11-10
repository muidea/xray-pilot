//! User-facing paths only; never replace canonical paths used for file access.
use std::path::Path;

pub fn path(path: &Path) -> String {
    text(&path.to_string_lossy())
}

pub fn text(path: &str) -> String {
    #[cfg(windows)]
    {
        windows_text(path)
    }
    #[cfg(not(windows))]
    {
        path.into()
    }
}

#[cfg(any(windows, test))]
fn windows_text(path: &str) -> String {
    let native = path.replace('/', r"\");
    if let Some(rest) = native.strip_prefix(r"\\?\") {
        if rest
            .get(..4)
            .is_some_and(|s| s.eq_ignore_ascii_case(r"UNC\"))
        {
            return format!(r"\\{}", &rest[4..]);
        }
        let bytes = rest.as_bytes();
        if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && &bytes[1..3] == b":\\" {
            return rest.into();
        }
    }
    // Volume GUID and device namespaces have no equivalent ordinary drive path.
    native
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_paths_show_drive_letters_and_unc_shares() {
        for (input, expected) in [
            (r"\\?\D:\Xray Pilot\数据\", r"D:\Xray Pilot\数据\"),
            (r"\\?\UNC\server\share\资源", r"\\server\share\资源"),
            (r"\\?\unc\server\share\资源", r"\\server\share\资源"),
            ("D:/Xray Pilot/data", r"D:\Xray Pilot\data"),
            (r"\\server\share\data", r"\\server\share\data"),
            (r"\\?\Volume{guid}\data", r"\\?\Volume{guid}\data"),
            (r"\\.\pipe\manager", r"\\.\pipe\manager"),
            ("", ""),
        ] {
            assert_eq!(windows_text(input), expected);
        }
    }

    #[test]
    fn native_display_keeps_the_original_path_value() {
        let original = Path::new(r"\\?\D:\数据\folder.\xray.exe");
        let before = original.to_path_buf();
        let shown = path(original);
        assert_eq!(original, before);
        #[cfg(windows)]
        assert_eq!(shown, r"D:\数据\folder.\xray.exe");
        #[cfg(not(windows))]
        assert_eq!(shown, original.to_string_lossy());
    }
}
