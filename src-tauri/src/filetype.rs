//! What kind of file a name is, by its extension: the space map colors by these, and the file
//! types page groups extensions under them. The UI holds the names and colors, by index.

/// How many kinds there are; [`kind_of`] returns an index below this.
pub const KINDS: usize = 10;

pub const VIDEO: u8 = 0;
pub const IMAGE: u8 = 1;
pub const AUDIO: u8 = 2;
/// Archives and installers: things that were downloaded to be unpacked or run once.
pub const ARCHIVE: u8 = 3;
/// Disk images and virtual machine disks.
pub const DISK: u8 = 4;
pub const PROGRAM: u8 = 5;
/// Source code, build output and developer tool data.
pub const DEV: u8 = 6;
pub const DOCUMENT: u8 = 7;
/// Windows' own: the page file, logs, component store manifests.
pub const SYSTEM: u8 = 8;
pub const OTHER: u8 = 9;

/// The extension of `name`, lowercased, without the dot; empty when there is none worth the name
/// (no dot, a leading dot only, or something too long or odd to be an extension).
pub fn ext_of(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => {
            let ext = &name[i + 1..];
            if ext.len() <= 12
                && ext
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            {
                ext.to_ascii_lowercase()
            } else {
                String::new()
            }
        }
        _ => String::new(),
    }
}

/// The kind of a file with extension `ext` (as [`ext_of`] gives it).
pub fn kind_of(ext: &str) -> u8 {
    match ext {
        "mp4" | "mkv" | "avi" | "mov" | "wmv" | "flv" | "webm" | "m4v" | "rmvb" | "rm" | "mpg"
        | "mpeg" | "m2ts" | "mts" | "3gp" | "vob" | "f4v" => VIDEO,
        "jpg" | "jpeg" | "png" | "gif" | "bmp" | "webp" | "heic" | "heif" | "tif" | "tiff"
        | "raw" | "cr2" | "cr3" | "nef" | "arw" | "dng" | "psd" | "ico" | "svg" | "avif"
        | "jfif" => IMAGE,
        "mp3" | "flac" | "wav" | "aac" | "m4a" | "ogg" | "opus" | "wma" | "ape" | "mid"
        | "midi" | "aiff" => AUDIO,
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "lz4" | "cab"
        | "msi" | "msix" | "msixbundle" | "appx" | "appxbundle" | "apk" | "aab" | "xapk"
        | "ipa" | "dmg" | "pkg" | "deb" | "rpm" | "nupkg" => ARCHIVE,
        "iso" | "img" | "vhd" | "vhdx" | "avhdx" | "vmdk" | "vdi" | "qcow2" | "wim" | "esd"
        | "swm" | "hds" => DISK,
        "exe" | "dll" | "ocx" | "drv" | "efi" | "mui" | "node" | "so" | "dylib" | "com" | "scr"
        | "cpl" | "ax" | "winmd" | "tlb" => PROGRAM,
        "js" | "mjs" | "cjs" | "ts" | "tsx" | "jsx" | "py" | "pyc" | "pyd" | "rs" | "rlib"
        | "rmeta" | "go" | "java" | "class" | "jar" | "kt" | "kts" | "c" | "cc" | "cpp" | "h"
        | "hpp" | "cs" | "swift" | "dart" | "rb" | "php" | "lua" | "obj" | "o" | "a" | "lib"
        | "pdb" | "ilk" | "pch" | "ipch" | "d" | "map" | "wasm" | "dex" | "gradle" | "lock"
        | "vsix" | "whl" | "gem" | "crate" | "idx" | "pack" | "sqlite" | "db" | "ldb" => DEV,
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "md" | "csv" | "rtf"
        | "odt" | "ods" | "odp" | "epub" | "mobi" | "wps" | "et" | "dps" | "pages" | "numbers"
        | "key" | "chm" | "xps" => DOCUMENT,
        "sys" | "evtx" | "etl" | "mum" | "cat" | "manifest" | "pf" | "log" | "dmp" | "edb"
        | "jrs" | "chk" | "blf" | "regtrans-ms" => SYSTEM,
        _ => OTHER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_and_kinds() {
        assert_eq!(ext_of("Movie.MP4"), "mp4");
        assert_eq!(ext_of(".gitignore"), "");
        assert_eq!(ext_of("README"), "");
        assert_eq!(ext_of("a.tar.gz"), "gz");
        assert_eq!(ext_of("v1.2 final copy"), "");
        assert_eq!(kind_of(&ext_of("pagefile.sys")), SYSTEM);
        assert_eq!(kind_of(&ext_of("setup.msi")), ARCHIVE);
        assert_eq!(kind_of(&ext_of("ubuntu.vhdx")), DISK);
        assert_eq!(kind_of(&ext_of("x.unknownthing")), OTHER);
    }
}
