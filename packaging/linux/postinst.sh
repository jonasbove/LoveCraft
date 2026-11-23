#!/bin/sh
# deb/rpm post-install and post-remove: refresh the desktop, MIME and icon caches, so the menu
# shows LoveCraft and file managers offer it for .prosecraft backups (the desktop entry's
# MimeType=application/x-prosecraft, defined in /usr/share/mime/packages/). Every tool is
# optional; missing ones are skipped. Per-user choices (xdg-mime default) are left to the person:
# a system package doesn't write into home folders.
# Adapted from CraftCloud's packaging/linux/postinst.sh (MIT OR Apache-2.0; see NOTICE).
if command -v update-mime-database >/dev/null 2>&1; then
  update-mime-database /usr/share/mime || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
exit 0
