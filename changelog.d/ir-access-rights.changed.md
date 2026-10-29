- `lox config push` stages a `permissions.bin` generated for the compiled config when lxir reproduces the live
  one byte for byte, so access granted or revoked in the module reaches it. Otherwise the live file stays, and
  the push warns when the compiled access rights no longer match it. The JSON report has `permissions`
  (`unchanged`, `regenerated`, `kept`, `absent`) and `permissions_stale`.
