# D70: Users stay the Miniserver's

Status: accepted (2026-10-31)

## Context

Users, user groups, passwords, NFC tags and access codes live in the program file, but the Miniserver's
user management changes them outside it (see
[loxone-format.md](../loxone-format.md#users-groups-and-access-rights)). Issue #194 asked for users and
operating modes from source; the modes are [D69](0069-operating-modes.md).

## Decision

The Miniserver's user management owns users, groups and credentials, never the source. The source names
users where logic needs them: access rights on an NFC Code Touch ([D49](0049-nfc-access-rights.md)) and
notification recipients ([D51](0051-push-notifications.md)). The compiler refuses a name the base config
does not hold.

Reasons:

- **Credentials do not belong in a repository.** A password or access code in a module would be in every
  clone and every commit.
- **The Miniserver changes users outside the program file.** The app and `lox user` write the user
  management. It keeps the change apart from the program file until the program is saved again, and drops
  it when an uploaded program loads. So `lox config push` compares the live users with those of the
  program file it compiles against, and refuses an upload that would discard a change
  (`src/commands/push_users.rs`). A source that owned users would fight that guard on every push: whatever
  the app changed since the last download would read as drift to overwrite.
- **Who lives in the house is not automation.** The rights a user has at a door are, and they stay claims
  ([D49](0049-nfc-access-rights.md)). The user is not.

## Rejected

- Users declared in source, with secrets taken from the environment. The push guard and the app would
  still disagree about who owns a change.
- Users declared without secrets. A user whose password the source cannot set is a half-owned object.
