//! What a program file says about itself: its icon, description and publisher, and for a shortcut,
//! what it points at. All in one PowerShell batch, since .NET reads version resources, shortcuts
//! and icons without any COM plumbing here.

use crate::apps::powershell;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct FileDetails {
    /// PNG data URL.
    pub icon: Option<String>,
    pub description: Option<String>,
    pub company: Option<String>,
    /// For a .lnk: the file it opens.
    pub target: Option<String>,
}

#[derive(Deserialize, Default)]
struct Raw {
    icon: Option<String>,
    description: Option<String>,
    company: Option<String>,
    target: Option<String>,
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

#[derive(Serialize, Deserialize, Clone)]
struct Cached {
    modified: u64,
    len: u64,
    details: FileDetails,
}

const CACHE: &str = "files.json";
static MEM: Mutex<Option<HashMap<String, Cached>>> = Mutex::new(None);

/// What identifies this version of a file: when it was last written, and its length.
fn stamp(path: &str) -> Option<(u64, u64)> {
    let md = std::fs::metadata(path).ok()?;
    Some((
        md.modified().map(crate::util::unix_secs).unwrap_or(0),
        md.len(),
    ))
}

/// Details for each path, keyed by the path as given. Paths that cannot be read are left out.
///
/// Remembered on disk per file and reused while the file is unchanged, so only programs not seen
/// before cost a PowerShell run.
pub fn details(paths: &[String]) -> HashMap<String, FileDetails> {
    let mut out = HashMap::new();
    let mut missing = Vec::new();
    {
        let mut mem = MEM.lock().unwrap();
        let cache = mem.get_or_insert_with(|| crate::cache::load(CACHE).unwrap_or_default());
        for p in paths {
            match (stamp(p), cache.get(p)) {
                (Some((m, l)), Some(c)) if c.modified == m && c.len == l => {
                    out.insert(p.clone(), c.details.clone());
                }
                (Some(_), _) => missing.push(p.clone()),
                (None, _) => {}
            }
        }
    }
    if missing.is_empty() {
        return out;
    }
    let found = query(&missing);
    let mut mem = MEM.lock().unwrap();
    let cache = mem.get_or_insert_with(HashMap::new);
    for p in &missing {
        let d = found.get(p).cloned().unwrap_or_default();
        if let Some((modified, len)) = stamp(p) {
            cache.insert(
                p.clone(),
                Cached {
                    modified,
                    len,
                    details: d.clone(),
                },
            );
        }
        out.insert(p.clone(), d);
    }
    crate::cache::save(CACHE, &*cache);
    out
}

/// Asks PowerShell about `paths`, all in one run.
fn query(paths: &[String]) -> HashMap<String, FileDetails> {
    if paths.is_empty() {
        return HashMap::new();
    }
    let list = std::env::temp_dir().join(format!("diskmate-files-{}.json", std::process::id()));
    if std::fs::write(&list, serde_json::to_vec(paths).unwrap_or_default()).is_err() {
        return HashMap::new();
    }
    let script = format!(
        r#"Add-Type -AssemblyName System.Drawing
$paths = Get-Content -Raw -Encoding UTF8 '{}' | ConvertFrom-Json
$sh = New-Object -ComObject WScript.Shell
$out = @{{}}
foreach ($q in $paths) {{
  $p = [string]$q
  $r = @{{}}
  $f = $p
  if ($p.ToLower().EndsWith('.lnk')) {{
    try {{ $t = $sh.CreateShortcut($p).TargetPath; if ($t) {{ $r.target = $t; $f = $t }} }} catch {{}}
  }}
  if (-not (Test-Path -LiteralPath $f -PathType Leaf)) {{ $f = $p }}
  if (Test-Path -LiteralPath $f -PathType Leaf) {{
    try {{ $v = [Diagnostics.FileVersionInfo]::GetVersionInfo($f); $r.description = $v.FileDescription; $r.company = $v.CompanyName }} catch {{}}
    try {{
      $ico = [System.Drawing.Icon]::ExtractAssociatedIcon($f)
      $bmp = $ico.ToBitmap(); $ms = New-Object IO.MemoryStream
      $bmp.Save($ms, [System.Drawing.Imaging.ImageFormat]::Png)
      $r.icon = [Convert]::ToBase64String($ms.ToArray())
    }} catch {{}}
  }}
  $out[$p] = $r
}}
$out | ConvertTo-Json -Compress -Depth 3"#,
        list.display().to_string().replace('\'', "''")
    );
    let raw: HashMap<String, Raw> = powershell(&script)
        .and_then(|json| serde_json::from_str(json.trim()).ok())
        .unwrap_or_default();
    let _ = std::fs::remove_file(&list);
    raw.into_iter()
        .map(|(p, r)| {
            (
                p,
                FileDetails {
                    icon: non_empty(r.icon).map(|b| format!("data:image/png;base64,{b}")),
                    description: non_empty(r.description),
                    company: non_empty(r.company),
                    target: non_empty(r.target),
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod real {
    /// `cargo test --release real_filemeta -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_filemeta() {
        let p = r"C:\WINDOWS\system32\ctfmon.exe".to_string();
        let d = super::details(&[p.clone()]);
        println!(
            "{:?}",
            d.get(&p)
                .map(|d| (&d.description, &d.company, d.icon.as_ref().map(|i| i.len())))
        );
    }
}
