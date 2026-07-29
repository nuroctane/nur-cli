---
name: cybersecurity
description: "Router into 817 Anthropic-Cybersecurity-Skills (MITRE ATT&CK, NIST CSF, ATLAS, D3FEND, AI RMF, F3). Use for security investigations, DFIR, red/blue team playbooks."
---

# Cybersecurity skills library

Source: https://github.com/mukul975/Anthropic-Cybersecurity-Skills (Apache-2.0, community).

**Authorized & lawful use only.** Offensive skills are for systems you own or have written permission to test.

## How Meta uses this pack

- Full skill bodies live under `~/.agents/skills/` (and mirrors) after ecosystem ensure.
- Do **not** load all 817 into context. Progressive disclosure:
  1. Match the user task to a skill **name** via list/grep of skill dirs or index.
  2. `skill(action=read, name=<kebab-name>)` for the full playbook.
  3. Execute workflow steps with bash/read tools; map findings to ATT&CK IDs.

## Domains (29)

Cloud · Threat Hunting · Threat Intel · Network · Web App · DFIR · Malware · IAM · SOC · Red Team · Containers · OT/ICS · API · IR · Vuln Mgmt · Pentest · DevSecOps · Zero Trust · Endpoint · Crypto · Phishing · AI Security · Mobile · Ransomware · Compliance · Supply Chain · Deception · Hardware/Firmware

## Example matches

| User ask | Skill to load |
|----------|----------------|
| memory dump credential theft | performing-memory-forensics-with-volatility3 |
| S3 public buckets | auditing-aws-s3-bucket-permissions |
| prompt injection | detecting-ai-model-prompt-injection-attacks |
| kerberoasting | detecting-kerberoasting-attacks |

Index: https://raw.githubusercontent.com/mukul975/Anthropic-Cybersecurity-Skills/main/index.json
