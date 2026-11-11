# Security

## Reporting a problem

Report a vulnerability privately: open the repository's **Security** tab and choose **Report a vulnerability**.
Please don't open a public issue for it.

Only the latest release gets fixes. Every merge to main is released, so a fix ships as a new version.

## What lox protects

lox holds the credentials of your Miniserver and can switch everything it controls.

- `~/.lox/config.yaml` and the token file are readable by your user only.
- `lox mcp` asks for confirmation before risky actions, such as doors, gates and alarms. `--read-only` removes
  every action.
- Passwords, tokens and access codes are masked in logs and errors.

## Known limits

- **TLS certificates are not verified by default.** Miniservers use self-signed certificates. Set
  `verify_ssl: true` when yours has a valid one.
- **FTP is unencrypted.** The config commands (`lox config pull`, `push`, …) send the admin password over FTP in
  the clear, because that is the only way the Miniserver offers its program file. Use them on a network you
  trust.
- **The password is stored in plain text** in `config.yaml`. Use a dedicated Miniserver user with only the
  rights lox needs.
