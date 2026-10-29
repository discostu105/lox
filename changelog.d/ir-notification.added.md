- **Push notifications from source**: in a `.lxir` module, `OutputRefLM(mirrors: <notification>, AI: …,
  On: "…", Off: "…", MinimumTime: 12h)` draws a push notification on a page with its message, and a
  `recipients <notification> … end` declaration lists the users and groups that get it, by title
  (`user "alice"`). The notification itself stays with Loxone Config (an extern `Notification(…)`). Message
  texts show in `lox ir diff`, and the semantic fingerprint now covers them (`lox ir drift` still accepts
  fingerprints recorded before).
