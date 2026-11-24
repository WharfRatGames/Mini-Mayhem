//! Experimental-channel OTA helpers: pure functions (no I/O, no globals) so every decision the updater makes is unit-tested.
//!
//! Published files (Pi nginx, `/var/www/html/arty/exp/`), written by `exp-deploy/publish_exp_ota.sh`, version file LAST:
//!   `arty-exp`      the Miyoo binary
//!   `changelog.txt` update-screen notes
//!   `version.txt`   `<version> <size-bytes> <sha256-hex>` on one line
//! A client only installs a binary whose size AND sha256 match the version line, and only when that version is newer.

use sha2::{Digest, Sha256};

/// URL prefix of the experimental channel (the production channel is `/arty`).
pub const EXP_BASE: &str = "/arty/exp";
/// Binary name on the server and on the device.
pub const EXP_BIN: &str = "arty-exp";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OtaInfo {
    pub version: String,
    pub size: u64,
    pub sha256: String,
}

/// Parse `version.txt`. Strict on purpose: an nginx 404 page, an empty body or a half-written file must never look like an update.
pub fn parse_version_file(body: &[u8]) -> Option<OtaInfo> {
    let text = std::str::from_utf8(body).ok()?;
    let line = text.lines().map(str::trim).find(|l| !l.is_empty())?;
    let mut it = line.split_whitespace();
    let version = it.next()?.to_string();
    let size: u64 = it.next()?.parse().ok()?;
    let sha256 = it.next()?.to_ascii_lowercase();
    if it.next().is_some() { return None; }
    if parse_version(&version).is_none() { return None; }
    if size < 1_000_000 { return None; } // a real build is megabytes; guards truncated/garbage metadata
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) { return None; }
    Some(OtaInfo { version, size, sha256 })
}

/// `0.5.4.439-exp37` -> ([0,5,4,439], 37). Plain release versions (no `-exp`) get exp number 0.
pub fn parse_version(v: &str) -> Option<(Vec<u32>, u32)> {
    let (base, exp) = match v.split_once("-exp") {
        Some((b, e)) => (b, e.parse().ok()?),
        None => (v, 0),
    };
    let nums: Option<Vec<u32>> = base.split('.').map(|p| p.parse().ok()).collect();
    let nums = nums?;
    if nums.len() < 2 { return None; }
    Some((nums, exp))
}

/// Offer an update only when the published version is strictly newer than the running one. Never downgrades (a stale or rolled-back
/// channel file must not undo a newer manual push) and never loops on an equal version.
pub fn is_newer(server: &str, current: &str) -> bool {
    match (parse_version(server), parse_version(current)) {
        (Some(s), Some(c)) => s > c,
        _ => false,
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data).iter().map(|b| format!("{:02x}", b)).collect()
}

/// Does a downloaded file match what `version.txt` promised?
pub fn verify(data: &[u8], info: &OtaInfo) -> bool {
    data.len() as u64 == info.size && sha256_hex(data) == info.sha256
}

/// Streaming variant of `verify` for a file on disk (the binary is tens of MB; the Miyoo should not hold it in RAM to hash it).
pub fn verify_file(path: &std::path::Path, info: &OtaInfo) -> bool {
    use std::io::Read;
    let mut f = match std::fs::File::open(path) { Ok(f) => f, Err(_) => return false };
    let mut h = Sha256::new();
    let mut total = 0u64;
    let mut chunk = [0u8; 65536];
    loop {
        match f.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => { h.update(&chunk[..n]); total += n as u64; }
            Err(_) => return false,
        }
    }
    let hex: String = h.finalize().iter().map(|b| format!("{:02x}", b)).collect();
    total == info.size && hex == info.sha256
}

/// Shell script that replaces the running binary and relaunches it. Run in place of the game process (execv), so it is also the
/// parent of the new game and can roll back a build that dies at start-up.
///  * the new binary is copied next to the old one and size-checked BEFORE it is renamed over it (rename is atomic: a power cut or a
///    full SD card can never leave a half-written `arty-exp`); the old binary is kept as `<dest>.prev`
///  * `.ota_pending` marks "first run of an update"; the game deletes it after 20 s of healthy running (`ota::BOOT_OK_SECS`)
///  * if the new build exits abnormally within 20 s while the marker exists, `.prev` is restored and relaunched. A normal exit, or a
///    kill by the push script (SIGTERM/SIGKILL), is left alone.
///  * whatever happens the game is started again (the old binary if the copy failed), so an update can never leave the device on a
///    blank screen.
pub fn apply_script(dest: &str, new_file: &str, size: u64) -> String {
    format!(
r#"#!/bin/sh
D='{dest}'
N='{new_file}'
DIR=$(dirname "$D")
if cp "$N" "$D.new" && chmod 755 "$D.new" && [ "$(wc -c < "$D.new")" = "{size}" ]; then
  cp -f "$D" "$D.prev" 2>/dev/null
  echo 1 > "$DIR/.ota_pending"
  mv -f "$D.new" "$D"
else
  rm -f "$D.new"
fi
rm -f "$N"
T0=$(date +%s)
"$D"
RC=$?
T1=$(date +%s)
if [ -f "$DIR/.ota_pending" ] && [ $((T1-T0)) -lt {boot} ] && [ "$RC" -ne 0 ] && [ "$RC" -ne 143 ] && [ "$RC" -ne 137 ] && [ -f "$D.prev" ]; then
  rm -f "$DIR/.ota_pending"
  cp -f "$D.prev" "$D.rb" && mv -f "$D.rb" "$D"
  exec "$D"
fi
exit $RC
"#, dest = dest, new_file = new_file, size = size, boot = BOOT_OK_SECS)
}

/// Seconds the game must run after an update before the update is considered good (and `.ota_pending` is cleared).
pub const BOOT_OK_SECS: u64 = 20;

/// The update gate is skipped (prevents retry loops) only when the last attempt did NOT end with the target version running.
/// `sentinel` = contents of the attempt marker, if any. After a successful update the marker equals the running version, so the
/// next published build is offered normally.
pub fn gate_skipped(sentinel: Option<&str>, running: &str) -> bool {
    match sentinel {
        Some(s) => s.trim() != running,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "aa23be22d0a0e4a1b9b2c3d4e5f60718293a4b5c6d7e8f90112233445566778a";

    #[test]
    fn version_file_roundtrip() {
        let i = parse_version_file(format!("0.5.4.439-exp38 5242880 {SHA}\n").as_bytes()).unwrap();
        assert_eq!(i.version, "0.5.4.439-exp38");
        assert_eq!(i.size, 5_242_880);
        assert_eq!(i.sha256, SHA);
    }

    #[test]
    fn garbage_is_never_an_update() {
        for bad in ["", "\n", "<html>404 Not Found</html>", "0.5.4.439-exp38", "0.5.4.439-exp38 100 abc",
                    "0.5.4.439-exp38 5242880", "0.5.4.439-exp38 notanumber ", "x.y.z 5242880 aa",
                    "0.5.4.439-exp38 999 aa23be22d0a0e4a1b9b2c3d4e5f60718293a4b5c6d7e8f90112233445566778a",
                    "0.5.4.439-exp38 5242880 aa23be22d0a0e4a1b9b2c3d4e5f60718293a4b5c6d7e8f90112233445566778a extra"] {
            assert!(parse_version_file(bad.as_bytes()).is_none(), "{bad:?}");
        }
        assert!(parse_version_file(&[0xff, 0xfe, 0x00]).is_none());
    }

    #[test]
    fn only_strictly_newer_versions_are_offered() {
        assert!(is_newer("0.5.4.439-exp38", "0.5.4.439-exp37"));
        assert!(is_newer("0.5.4.439-exp100", "0.5.4.439-exp99")); // numeric, not text, order
        assert!(is_newer("0.5.4.440-exp1", "0.5.4.439-exp37"));
        assert!(!is_newer("0.5.4.439-exp37", "0.5.4.439-exp37"));
        assert!(!is_newer("0.5.4.439-exp36", "0.5.4.439-exp37"), "never downgrade");
        assert!(!is_newer("0.5.4.438-exp99", "0.5.4.439-exp37"), "never downgrade");
        assert!(!is_newer("garbage", "0.5.4.439-exp37"));
        assert!(!is_newer("0.5.4.439-exp38", "garbage"));
    }

    #[test]
    fn verify_checks_size_and_hash() {
        let data = vec![7u8; 2_000_000];
        let info = OtaInfo { version: "0.5.4.439-exp38".into(), size: data.len() as u64, sha256: sha256_hex(&data) };
        assert!(verify(&data, &info));
        let mut flipped = data.clone(); flipped[1_000_000] ^= 1;
        assert!(!verify(&flipped, &info), "one flipped bit must fail");
        assert!(!verify(&data[..data.len() - 1], &info), "truncated must fail");
        let p = std::env::temp_dir().join(format!("ota_verify_{}.bin", std::process::id()));
        std::fs::write(&p, &data).unwrap();
        assert!(verify_file(&p, &info));
        std::fs::write(&p, &flipped).unwrap();
        assert!(!verify_file(&p, &info));
        std::fs::write(&p, &data[..1000]).unwrap();
        assert!(!verify_file(&p, &info));
        let _ = std::fs::remove_file(&p);
        assert!(!verify_file(std::path::Path::new("/nonexistent/x"), &info));
    }

    #[test]
    fn gate_skip_only_after_a_failed_attempt() {
        assert!(!gate_skipped(None, "0.5.4.439-exp38"));
        assert!(!gate_skipped(Some("0.5.4.439-exp38"), "0.5.4.439-exp38"), "update took: offer the next one");
        assert!(gate_skipped(Some("0.5.4.439-exp38"), "0.5.4.439-exp37"), "update did not take: do not loop");
    }

    #[test]
    fn script_is_atomic_and_always_relaunches() {
        let s = apply_script("/mnt/SDCARD/Roms/PORTS/Games/ArtyExp/arty-exp", "/tmp/arty-exp.new", 123456);
        assert!(s.contains("mv -f \"$D.new\" \"$D\""));
        assert!(s.contains("\"123456\""));
        assert!(s.contains("else\n  rm -f \"$D.new\""), "a failed copy must clean up and still run the old binary");
        assert!(s.contains("exec \"$D\""));
        assert!(s.find("mv -f \"$D.new\"").unwrap() < s.find("T0=").unwrap());
        // the script itself must be valid sh
        let tmp = std::env::temp_dir().join(format!("ota_script_test_{}.sh", std::process::id()));
        std::fs::write(&tmp, s).unwrap();
        let st = std::process::Command::new("sh").arg("-n").arg(&tmp).status().unwrap();
        let _ = std::fs::remove_file(&tmp);
        assert!(st.success(), "sh -n rejected the script");
    }

    /// Run the real script against a fake game directory: success path, failed-copy path and crash rollback.
    #[test]
    fn script_behaviour_in_a_sandbox() {
        let dir = std::env::temp_dir().join(format!("ota_sandbox_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("arty-exp");
        let newf = dir.join("incoming.bin");
        let log = dir.join("log.txt");
        let mk = |p: &std::path::Path, body: &str| {
            std::fs::write(p, body).unwrap();
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        let run = |size: u64| {
            let sc = dir.join("apply.sh");
            std::fs::write(&sc, apply_script(dest.to_str().unwrap(), newf.to_str().unwrap(), size)).unwrap();
            std::process::Command::new("sh").arg(&sc).status().unwrap().code().unwrap()
        };
        // 1. good update: new build runs, old one kept as .prev, marker present (the "game" never clears it here)
        mk(&dest, &format!("#!/bin/sh\necho old >> {}\nexit 0\n", log.display()));
        let good = format!("#!/bin/sh\necho new >> {}\nexit 0\n", log.display());
        mk(&newf, &good);
        assert_eq!(run(good.len() as u64), 0);
        assert!(std::fs::read_to_string(&log).unwrap().trim_end().ends_with("new"));
        assert!(dir.join("arty-exp.prev").exists());
        assert!(!newf.exists(), "incoming file is removed");
        // 2. wrong size (truncated copy): the old binary keeps running, nothing is replaced
        let _ = std::fs::remove_file(&log); let _ = std::fs::remove_file(dir.join(".ota_pending"));
        mk(&dest, &format!("#!/bin/sh\necho old >> {}\nexit 0\n", log.display()));
        mk(&newf, &good);
        assert_eq!(run(good.len() as u64 + 5), 0);
        assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), "old");
        assert!(!dir.join("arty-exp.new").exists());
        // 3. new build crashes at start: rolled back to the previous binary, which then runs
        let _ = std::fs::remove_file(&log); let _ = std::fs::remove_file(dir.join(".ota_pending"));
        mk(&dest, &format!("#!/bin/sh\necho old >> {}\nexit 0\n", log.display()));
        let bad = format!("#!/bin/sh\necho crashed >> {}\nexit 139\n", log.display());
        mk(&newf, &bad);
        assert_eq!(run(bad.len() as u64), 0, "rollback relaunches the old build, which exits 0");
        let l = std::fs::read_to_string(&log).unwrap();
        assert_eq!(l.lines().collect::<Vec<_>>(), vec!["crashed", "old"]);
        assert!(!dir.join(".ota_pending").exists());
        // 4. a SIGTERM-style exit (push script killall) within 20 s is NOT treated as a crash
        let _ = std::fs::remove_file(&log);
        mk(&dest, &format!("#!/bin/sh\necho old >> {}\nexit 0\n", log.display()));
        let killed = format!("#!/bin/sh\necho killed >> {}\nexit 143\n", log.display());
        mk(&newf, &killed);
        assert_eq!(run(killed.len() as u64), 143);
        assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), "killed");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
