---
name: deploy-check
description: Verify the deployed build before testing or walking a live site. Use when asked to check a deployment, test a live site, or confirm what version is actually running.
---

# Deploy check

Never test a deployment you have not fingerprinted. A test that passes
against the old build proves nothing about the new one.

1. **Fingerprint the build.** Find the version the deployment reports:
   a meta tag in the page, a `/__version` endpoint, a build id in the
   response headers, or the release the deploy pipeline names. Record
   the exact fingerprint, not "the latest".
2. **Compare with the expected build.** The fingerprint must match the
   commit, tag, or release the user expects to be live. If it does not
   match, stop: the deployment has not landed, or the test is aimed at
   the wrong environment. Report the mismatch before any further testing.
3. **Then test.** Only with a matching fingerprint, run the checks the
   user asked for against the live deployment. Record which fingerprint
   the results belong to, so a later deploy does not silently invalidate
   them.
