# Agent instructions — meta-cli

## Ship / push / deploy (mandatory)

Whenever the user says **ship**, **push**, **deploy**, **main**, **release**, **publish**, or similar:

**Follow `C:\Users\david\.agents\SHIP.md` (meta-cli section).**  
**Or:** `pwsh -File $env:USERPROFILE\.agents\ship.ps1 -Repo meta-cli -Message "…"`

Order for **this** repo (install is not optional):

1. **Commit** on `C:\Users\david\Laboratory\meta-cli`
2. **Install** `target\release\meta.exe` → `%USERPROFILE%\.local\bin\meta.exe` (+ `muse.exe`); confirm `meta --version`
3. **Push** `git push origin main`
4. **Backup** 7z → `D:\BACKUP\CODE Backups\meta-cli\`  
   Name: `meta-cli_YYYY-MM-DD_<sha>_<slug>.7z`

Do not skip install or backup. Report all four outcomes.  
Canonical process + other agents: `C:\Users\david\.agents\AGENTS.md`
