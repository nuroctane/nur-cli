# Agent instructions — meta-cli

## Ship / push / deploy (mandatory)

Whenever the user says **ship**, **push**, **deploy**, **main**, **release**, or similar:

**Follow `C:\Users\david\.agents\SHIP.md` (meta-cli section).**

Order for **this** repo:

1. Commit on `C:\Users\david\Laboratory\meta-cli`
2. **Install** `target\release\meta.exe` → `%USERPROFILE%\.local\bin\meta.exe` (+ `muse.exe`)
3. `git push origin main`
4. Backup 7z → `D:\BACKUP\CODE Backups\meta-cli\` (name: `meta-cli_YYYY-MM-DD_<sha>_<slug>.7z`)

Do not skip install or backup. Report all four outcomes.
