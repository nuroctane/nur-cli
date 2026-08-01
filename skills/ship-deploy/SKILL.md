---
name: ship-deploy
description: >
  MANDATORY ship/push/deploy pipeline for EVERY Laboratory repo. Triggers on:
  ship, push, deploy, put on main, land on main, merge to main, release, publish,
  sync main, install it (after code), backup after ship, or any request to put work
  on main / on the user's machine. NEVER bare git-push: always commit (if needed),
  push origin main, then 7z backup to D:\BACKUP\CODE Backups. nur-cli adds system
  install of nur.exe after backup. Read C:\Users\david\.agents\SHIP.md and/or run
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

### nur-cli only

1. **Commit**  
2. **Push** `origin main`
3. **Backup** as above under `nur-cli\`
4. **Install** `target\release\nur.exe` to `%USERPROFILE%\.local\bin\nur.exe`; verify `nur --version`

## Never

- Push without backup  
- Ship nur-cli without system install
- Claim â€œdoneâ€ without reporting commit, remote, install (for nur-cli), and **full** backup path
- Ask the user to restate this process  

## Done report template

```
Ship complete:
- commit: <sha> <subject>
- remote: origin/main
- install: <path/version>   # nur-cli only
- backup: D:\BACKUP\CODE Backups\<repo>\...
```

