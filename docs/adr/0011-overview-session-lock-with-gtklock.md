# ADR 0011: Use standalone GTKLock for the overview session lock

- **Status:** Accepted
- **Date:** 2026-08-14
- **Owners:** Foyer Shell project

## Context

ADR 0009 selected Hyprlock because it implements the secure Wayland session-lock protocol, PAM
authentication, and a configurable overview-like presentation. Fedora 43 no longer ships Hyprlock.
The available third-party package couples it to the complete Hyprland compositor and a private
Hyprland library stack. Foyer Shell runs on Niri and should not install a second compositor merely to
obtain its locker.

GTKLock is a focused GTK3 application using `ext-session-lock-v1` and PAM. It accepts a custom
GtkBuilder layout and CSS, which are sufficient for the overview copy, live date and time, and the
bottom-centered password field. Fedora officially packages GTKLock and its small
`gtk-session-lock` library beginning with Fedora 44, so their reviewed source packages can be
rebuilt for Fedora 43 without mixing distribution releases.

## Decision

GTKLock becomes Foyer Shell's preferred lock backend. Foyer Shell maintains `config.ini`, `layout.ui`, and
`style.css` under `contrib/gtklock/` and links that directory into the user's standard GTKLock
configuration location. GTKLock owns the session-lock protocol, password entry, PAM conversation,
and unlock lifecycle; Foyer Shell owns only the static presentation files.

The Fedora 44 `gtk-session-lock` and GTKLock source packages are rebuilt unchanged on Fedora 43.
Only locally built binary RPMs are installed. This keeps package ownership, PAM configuration, and
uninstallation explicit while avoiding a third-party repository and another compositor.

All entry points retain the fixed `foyer-shell session lock` boundary. The service launches
GTKLock directly with explicit repository-managed config, layout, and style paths. It treats the
styled backend as available only when both the executable and layout exist. Swaylock remains the
secure recovery backend when GTKLock or its presentation files are unavailable. Once the
lock-aware Foyer Shell binary and GTKLock pass live validation, Niri routes `Super+Alt+L` through that
same typed action.

The overview paragraph and thoughtful line remain hardcoded for this slice. Later dynamic text must
arrive as a bounded, sanitized, plain-text snapshot produced before locking. Neither the layout nor
locker may run an agent, tools, network requests, mutable actions, or untrusted markup.

## Alternatives and deliberate exclusions

- Installing Hyprlock from a package that requires Hyprland adds an unrelated compositor and
  dependency stack.
- Installing Fedora 44 binary RPMs directly on Fedora 43 mixes release artifacts and bypasses a
  native rebuild.
- A normal GPUI or layer-shell overlay is not a secure lock and remains excluded.
- Forking swaylock or implementing PAM and `ext-session-lock-v1` inside Foyer Shell creates a larger
  security-sensitive maintenance surface.
- Third-party COPR repositories, live agent content, controls, notifications, and Presentations are
  excluded.

## Consequences and risks

The installed runtime is limited to GTKLock, `gtk-session-lock`, and ordinary GTK/PAM/Wayland
libraries already appropriate to the Niri session. The project must rebuild or upgrade two small
RPMs until the host moves to a Fedora release that packages them directly.

GTKLock's custom layout depends on stable object ids required by its window code. Package upgrades
must validate layout loading, authentication failure, multi-output behavior, and successful unlock
before activation. Dynamic copy still requires explicit privacy and escaping rules.

The Fedora 43 native rebuild and installation were completed on 2026-08-14. The repository layout
passed GtkBuilder validation and a live Niri lock/unlock pass before the compositor binding was
changed from direct swaylock to the typed Foyer Shell action.

## Validation criteria

- The Fedora 43 RPM build uses Fedora's official specs and checksum-verified upstream archives.
- Installing the local RPMs does not install Hyprland or replace Niri.
- GTKLock loads the repository layout and CSS without missing-object warnings.
- Niri enters a real locked state that ordinary application and compositor actions cannot bypass.
- Incorrect and empty credentials do not unlock; a correct PAM credential does.
- Date, time, overview copy, thoughtful copy, password input, Caps Lock state, and failure state render
  legibly on each output.
- The power panel and `Super+Alt+L` reach the same typed Foyer Shell action after activation.
- Removing GTKLock or its layout causes the typed action to fall back to swaylock.

## Supersession

This ADR supersedes ADR 0009's Hyprlock backend choice and extends ADR 0008's overview visual language.
It also supersedes ADR 0005 only where that record names swaylock as the primary presentation;
ADR 0005's typed execution, local-interaction, confirmation, and recovery principles remain.

ADR 0018 supersedes this record's former foyer terminology with Overview.
