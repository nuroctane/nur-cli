---
name: ship-deploy
description: >
  MANDATORY ship/push/deploy pipeline for EVERY Laboratory repo. Triggers on:
  ship, push, deploy, put on main, land on main, merge to main, release, publish,
  sync main, install it (after code), backup after ship, or any request to put work
  on main / on the user's machine. NEVER bare git-push: always commit (if needed),
  push origin main, then 7z backup to D:\BACKUP\CODE Backups. meta-cli adds system
  install of meta.exe before push. Read C:\Users\david\.agents\SHIP.md and/or run
  ship.ps1. User should never re-explain this.
---

# Ship / push / deploy (mandatory)

## Do this first

1. Read and follow: `C:\Users\david\.agents\SHIP.md`
2. Prefer automation when appropriate:

```powershell
powershell -File $env:USERPROFILE\.agents\ship.ps1 -Repo <repo-name> -Message "commit message"
# already committed:
powershell -File $env:USERPROFILE\.agents\ship.ps1 -Repo <repo-name> -SkipCommit
```

## Short form

### Default repo (`Laboratory/<repo>`)

1. **Commit** (if dirty)
2. **Push** `origin main`
3. **Backup** 7z â†’ `D:\BACKUP\CODE Backups\<repo>\`  
   Pattern: `<repo>_YYYY-MM-DD_<sha>_<subject-slug>.7z`  
   Exclude: `target`, `.git`, `node_modules`, `graphify-out`, `.next`, `dist`

### meta-cli only

1. **Commit**  
2. **Install** `target\release\meta.exe` â†’ `%USERPROFILE%\.local\bin\meta.exe` (+ `muse.exe`); verify `meta --version`  
3. **Push** `origin main`  
4. **Backup** as above under `meta-cli\`

## Never

- Push without backup  
- Ship meta-cli without system install  
- Claim â€œdoneâ€ without reporting commit, remote, install (if meta), and **full** backup path  
- Ask the user to restate this process  

## Done report template

```
Ship complete:
- commit: <sha> <subject>
- remote: origin/main
- install: <path/version>   # meta-cli only
- backup: D:\BACKUP\CODE Backups\<repo>\...
```

