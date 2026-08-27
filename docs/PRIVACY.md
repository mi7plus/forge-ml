# Diagnostics and privacy

Forge ML diagnostics are disabled by default. Enabling **Record bounded local diagnostics** in Settings applies to the current project root and stores files under `.forge/diagnostics`.

Recorded events contain a timestamp, event category, Forge version, operating system, and architecture. Crash summaries additionally contain a sanitized panic message, source location, and thread name. Messages are length-limited; home-directory paths and common inline token, password, secret, and key assignments are redacted.

Forge does not record source files, notebook contents, datasets, SQL, environment variables, command output, credentials, tokens, or database/object-storage profile secrets. The event log rotates at 1 MB and diagnostic exports include at most 20 crash summaries.

Nothing is uploaded automatically. **Export reviewable diagnostics ZIP** creates a local archive selected by the user. The archive has a manifest stating what is included and excluded, and can be inspected with any ZIP tool before it is shared. Disabling diagnostics immediately stops new event and crash recording; existing local files remain user-controlled under `.forge/diagnostics`.
