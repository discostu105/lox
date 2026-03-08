# Loxone REST API Research — Neue Möglichkeiten für die CLI

## Bereits implementiert (Status Quo)

| Endpoint | Verwendung in `lox` |
|----------|-------------------|
| `GET /data/LoxApp3.json` | Structure cache (Controls, Rooms, etc.) |
| `GET /jdev/sps/io/{uuid}/{cmd}` | `lox send`, `lox on`, `lox off`, `lox pulse`, `lox blind`, `lox mood` |
| `GET /dev/sps/io/{uuid}/all` | `lox get`, `lox watch`, `lox ls --values` |
| `GET /dev/sps/io/{name}/state` | Input state (daemon) |
| `GET /dev/cfg/version` | `lox status` |
| `GET /dev/sys/heap` | `lox status` (Memory) |
| `GET /dev/sps/state` | `lox status` (PLC state) |
| `GET /dev/sys/check` | `lox status` (Connections) |
| `GET /data/status` | `lox status` (Name, IP, Online) |
| `GET /dev/fsget/log/def.log` | `lox log` |
| `WSS /ws/rfc6455` | Daemon (WebSocket) |

---

## NEUE Endpoints — Interessant für die CLI

### 1. Statistik / History (⭐ High Value)

| Endpoint | Beschreibung |
|----------|-------------|
| `GET /stats/statistics.json` | Übersicht aller gespeicherten Statistiken (welche Controls haben History) |
| `GET /stats/statistics.json/{controlUUID}` | Gefiltert pro Control (seit Firmware 6.1.10.16) |
| `GET /jdev/sps/getstatsdate` | Datum der statistics.json |
| `GET /binstatisticdata/{controlUUID}/{YYYYMM}` | Binäre Statistikdaten pro Monat |
| `GET /binstatisticdata/{controlUUID}/{YYYYMMDD}` | Binäre Statistikdaten pro Tag |

**Binärformat:** `ts` (Uint32, Sekunden seit 1.1.2009 Miniserver-Lokalzeit) + N × Float64 Values. Anzahl Values pro Eintrag steht in `LoxApp3.json` unter `control.statistic.outputs[]`.

**CLI-Ideen:**
- `lox history <control> [--month YYYY-MM] [--day YYYY-MM-DD]` — Statistikdaten holen und tabellarisch/CSV anzeigen
- `lox stats` — Alle Controls mit aktivierter Statistik auflisten
- Besonders nützlich für Energieverbrauch, Temperaturverläufe etc.

---

### 2. Wetter (⭐ High Value)

| Endpoint | Beschreibung |
|----------|-------------|
| `GET /data/weatheru.bin` | Gecachte Wetterdaten auf dem Miniserver (Binärformat, max 200 Einträge, je 108 Bytes) |
| `weather.loxone.com:6066/forecast/?user=loxone_{serial}&coord={lon},{lat}&asl={elevation}&format=1` | Loxone Weather Service (7 Tage, stündlich) |

**Binärformat weatheru.bin:** 108-Byte Chunks mit Timestamp, Temperatur, Luftfeuchtigkeit, Wind, Regen, etc.

**CLI-Ideen:**
- `lox weather` — Aktuelle Wettervorhersage anzeigen (aus Miniserver-Cache oder Cloud)
- `lox weather --forecast` — 7-Tage-Vorschau

---

### 3. Control-Erweiterungen

| Endpoint | Beschreibung |
|----------|-------------|
| `GET /jdev/sps/io/{uuid}/controlnotes` | Notizen/Hilfetext eines Controls (Plaintext) |
| `GET /jdev/sps/io/{uuid}/lockcontrol/{0,1}/{reason}` | Control sperren mit Grund (Admin, ab v11.3.2.11) |
| `GET /jdev/sps/io/{uuid}/unlockcontrol` | Control entsperren (Admin) |
| `GET /jdev/sps/io/{uuid}/securedDetails` | Verschlüsselte Details (Intercom, Kamera-URLs) |
| `GET /jdev/sps/ios/{hash}/{uuid}/{cmd}` | Gesicherter Befehl (Visualisierungspasswort) |
| `GET /jdev/sps/io/{name}/I1/on` | Spezifische Inputs von Funktionsblöcken setzen |
| `GET /jdev/sps/io/{name}/R` | Light Controller zurücksetzen |

**CLI-Ideen:**
- `lox lock <control> --reason "Wartung"` / `lox unlock <control>` — Controls sperren
- `lox get` könnte `controlnotes` mit anzeigen
- `lox send --secured` — Passwortgeschützte Controls bedienen

---

### 4. System & Diagnose

| Endpoint | Beschreibung |
|----------|-------------|
| `GET /jdev/cfg/api` | API-Info: MAC-Adresse, Config-Version, httpsStatus, hasEventSlots |
| `GET /jdev/cfg/apiKey` | API-Key für Hashing + httpsStatus |
| `GET /jdev/cfg/timezoneoffset` | Timezone-Offset des Miniservers |
| `GET /dev/sys/reboot` | Miniserver neustarten (⚠ braucht System-Rechte) |
| `GET /dev/sys/numtasks` | Anzahl System-Tasks |
| `GET /dev/sys/date` | System-Datum/Uhrzeit |
| `GET /dev/sps/changes` | PLC-Änderungen abfragen (Polling-Alternative) |
| `GET /jdev/sps/LoxAPPversion3` | Structure-File Version prüfen ohne volles Download |
| `GET /jdev/sys/checktoken/{hash}/{user}` | Token validieren ohne Verwendung |
| `GET /jdev/sys/killtoken/{hash}/{user}` | Token widerrufen |

**CLI-Ideen:**
- `lox status --extended` / `lox info` — Erweiterte Infos (MAC, Config-Version, TZ, Tasks)
- `lox reboot` — Miniserver neustarten (mit Bestätigung!)
- `lox time` — Systemzeit des Miniservers anzeigen
- `lox cache check` — Prüfen ob Cache aktuell ist ohne volles Download
- `lox token check` / `lox token revoke` — Token-Management erweitern

---

### 5. Structure File — Ungenutzte Daten

Die `LoxApp3.json` enthält weit mehr als aktuell genutzt wird:

| Feld | Beschreibung |
|------|-------------|
| `cats` | Kategorien (z.B. "Beleuchtung", "Beschattung") |
| `controls[].details` | Detaildaten pro Control (z.B. Mood-Listen bei LightController) |
| `controls[].states` | State-UUIDs für Sub-States (z.B. `activeMoods`, `colorList`) |
| `controls[].subControls` | Sub-Controls bei komplexen Typen (z.B. einzelne Dimmer in LightController) |
| `controls[].statistic` | Statistik-Konfiguration (Häufigkeit, Outputs) |
| `controls[].hasHistory` | Flag ob Control History unterstützt |
| `globalStates` | Globale States (Betriebsmodus, Sunrise/Sunset, etc.) |
| `weatherServer` | Wetter-Konfiguration (Koordinaten, Höhe) |
| `autopilot` | Autopilot-Regeln |
| `caller` | Caller-Service Konfiguration |
| `messageCenter` | Nachrichten-Zentrale |
| `times` | Timer-Konfiguration |

**CLI-Ideen:**
- `lox ls --cat <category>` — Controls nach Kategorie filtern
- `lox categories` — Alle Kategorien auflisten
- `lox info <control>` — Detailansicht mit Sub-Controls, States, Moods
- `lox globals` — Globale States anzeigen (Betriebsmodus, Sonnenaufgang etc.)

---

### 6. Task Recorder

| Endpoint | Beschreibung |
|----------|-------------|
| Task-System über `/jdev/sps/io/` | Zeitgesteuerte Befehls-Sequenzen |

Tasks = zeitgesteuerte Kommandos, die der Miniserver selbst ausführt. Jeder Task besteht aus einem oder mehreren Commands mit jeweils eigenem Zeitstempel. Tasks müssen per Polling aktuell gehalten werden (keine State-Updates).

**CLI-Ideen:**
- `lox task list` — Geplante Tasks anzeigen
- `lox task add` — Neuen zeitgesteuerten Befehl erstellen
- Ergänzung zum bestehenden Scene-System

---

### 7. Extensions & Geräte-Management

| Endpoint | Beschreibung |
|----------|-------------|
| Extension-Commands | Befehle für Loxone Extensions (braucht Full Access) |
| Air/Tree Device Commands | Befehle für Air/Tree Geräte per Seriennummer oder Name |

**CLI-Ideen:**
- `lox extensions` — Alle angeschlossenen Extensions auflisten
- `lox devices` — Air/Tree Devices auflisten mit Status

---

### 8. Discovery & Netzwerk

| Endpoint | Beschreibung |
|----------|-------------|
| `UDP:7070` (Broadcast 0x00) | Lokale Miniserver-Suche |
| `dns.loxonecloud.com/?getip&snr={serial}&json=true` | Öffentliche IP über Loxone Cloud DNS |
| `/upnp.xml` | UPnP Device Description |

**CLI-Ideen:**
- `lox discover` — Lokales Netzwerk nach Miniservern scannen (UDP Broadcast)
- `lox config set` Autodetect — Miniserver automatisch finden

---

### 9. Notifications & Services

| Endpoint | Beschreibung |
|----------|-------------|
| `mail.loxonecloud.com/sendmail/{serial}` | E-Mail über Loxone Mailer Service |
| `push.loxonecloud.com/v1/push` | Push-Notifications (HMAC-SHA1 signed) |
| `caller.loxone.com/cgi-bin/loxlive/call.pl` | Text-to-Speech Anruf-Service |

**CLI-Ideen:**
- `lox notify <message>` — Push-Notification senden
- Integration in Automations (Action: notify)

---

### 10. Music Server (Port 7091, inoffiziell)

| Endpoint | Beschreibung |
|----------|-------------|
| `GET /zone/{zone}/play` | Play/Pause/Stop Transport-Kontrolle |
| `GET /zone/{zone}/volume/{0-100}` | Lautstärke setzen |
| `GET /zone/{zone}/repeat/{0,1,2}` | Repeat-Modus (off/one/all) |
| `GET /zone/{zone}/shuffle/{0,1}` | Shuffle ein/aus |

**Achtung:** Nicht offiziell dokumentiert, aus Community Reverse-Engineering.

**CLI-Ideen:**
- `lox music play/pause/stop [zone]`
- `lox music volume <zone> <level>`

---

### 11. Firmware Updates

| Endpoint | Beschreibung |
|----------|-------------|
| `GET update.loxone.com/updatecheck.xml?serial={}&version={}` | Verfügbare Updates prüfen |
| `GET /jdev/sys/autoupdate` | Auto-Update triggern |
| `GET /jdev/sys/updatetolatestrelease` | Update auf neueste Version |

**CLI-Ideen:**
- `lox update check` — Prüfen ob neues Firmware verfügbar
- `lox update install` — Update triggern (mit Bestätigung)

---

### 12. Filesystem

| Endpoint | Beschreibung |
|----------|-------------|
| `GET /dev/fsget/<path>` | Dateien vom Miniserver-Filesystem lesen |
| FTP (Port 21) | Dateien lesen/schreiben (braucht FTP-Rechte) |

**CLI-Ideen:**
- `lox files ls [path]` — Miniserver-Filesystem browsen
- `lox files get <path>` — Datei herunterladen

---

## Priorisierte Empfehlung

### High Value, Low Effort (Quick Wins)

| Feature | Aufwand | Nutzen |
|---------|---------|--------|
| `lox categories` + `lox ls --cat` | Gering (Daten schon in LoxApp3.json) | Bessere Navigation |
| `lox globals` (Betriebsmodus, Sonnenaufgang) | Gering (Daten schon in LoxApp3.json) | Nützliche Infos |
| `lox cache check` (LoxAPPversion3) | Minimal | Smartere Cache-Validierung |
| Extended `lox info` (Sub-Controls, Notes) | Gering | Tiefere Einblicke |
| `lox status --extended` (MAC, TZ, Tasks) | Gering | Mehr System-Infos |

### High Value, Medium Effort

| Feature | Aufwand | Nutzen |
|---------|---------|--------|
| `lox history` / `lox stats` | Mittel (Binärformat parsen) | Sehr hoch — Energiedaten, Temperaturen |
| `lox weather` | Mittel (Binärformat 108B Chunks) | Hoch — Wettervorhersage |
| `lox discover` | Mittel (UDP Broadcast) | Setup vereinfachen |
| `lox lock` / `lox unlock` | Gering-Mittel | Admin-Funktionalität |
| `lox update check` | Gering | Firmware-Management |

### Lower Priority / Nische

| Feature | Aufwand | Nutzen |
|---------|---------|--------|
| `lox music` (inoffiziell) | Mittel | Music Server Steuerung |
| `lox reboot` | Minimal | Selten gebraucht |
| `lox notify` (Push/Mail) | Mittel (HMAC) | Nische |
| `lox files` (Filesystem) | Gering | Nische |

---

## Quellen

- [Loxone Web Services Doku](https://www.loxone.com/enen/kb/web-services/)
- [Communicating with the Miniserver (PDF)](https://www.loxone.com/wp-content/uploads/datasheets/CommunicatingWithMiniserver.pdf)
- [Structure File Doku (PDF)](https://www.loxone.com/wp-content/uploads/datasheets/StructureFile.pdf)
- [Inside-The-Loxone-Miniserver (Reverse Engineering)](https://github.com/sarnau/Inside-The-Loxone-Miniserver)
- [Statistik-Download Script](https://gist.github.com/sarnau/e859f2d7beae882476ce6b78a8ab59f1)
- [LoxWiki REST Webservice](https://loxwiki.atlassian.net/wiki/spaces/LOX/pages/1517355410/REST+Webservice)
- [XciD/loxone-ws (Go Library)](https://github.com/XciD/loxone-ws)
