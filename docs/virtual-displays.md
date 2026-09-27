# Virtual displays

Virtual displays are independent of ordinary screen capture. The CLI can manage compositor headless outputs on Linux. A user can stream an existing physical monitor without a virtual output or driver.

| OS | Native route considered | Current support |
| --- | --- | --- |
| Hyprland | `hyprctl` headless output through compositor IPC | Implemented + tested on one Hyprland host: create, list, and Ctrl-C removal. Browser capture of that output is unverified. |
| Sway | `swaymsg create_output` and safe removal of the created `HEADLESS-*` output | Implemented + awaiting physical-platform verification; no Sway live test. |
| KDE/GNOME/X11 | No adapter | NOT IMPLEMENTED. |
| Windows | Microsoft IddCx indirect display driver | NOT IMPLEMENTED; no driver source, installer, signature, or uninstall path. |
| macOS | No public general-purpose host display creation API identified | Blocked by documented OS limitation; physical capture remains the target. |

Hyprland documents `hyprctl output create headless <name>` in its [official guide](https://wiki.hypr.land/configuring/core/advanced-configuration/using-hyprctl/). The Linux module can also call Sway's `create_output` through [`swaymsg` IPC](https://github.com/swaywm/sway/blob/master/swaymsg/swaymsg.1.scd); both tools ship with their matching compositor. It records outputs created in the current process and compositor session, and refuses to remove others. The [ScreenCast portal](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html) defines a VIRTUAL source bit, but a portal implementation may not support it.

On the tested Hyprland host, `questdisplay virtual status` reported available, `virtual create` added a 1920×1080@60 `QUESTDISPLAY-*` output alongside the physical display, `virtual list` showed it, and Ctrl-C removed it. Run the create command in its own terminal and leave it running while using the virtual output:

```sh
questdisplay virtual status
questdisplay virtual create
# In another terminal:
questdisplay virtual list
```

The create command also handles SIGTERM and removes only the output it created in the current compositor session. A forced kill can leave an output that the next process will not claim; inspect the compositor's output list before removing such an orphan manually. The browser's display selector still exposes only `primary`. On this host the portal reported `AvailableSourceTypes=3`, meaning monitor and window but no VIRTUAL source bit. The current `scrcap` Linux adapter ignores a numeric monitor index and relies on the portal picker. A test attempted to select the new headless output through that picker but did not confirm frame delivery. Virtual-output capture is unverified; do not treat the successful create/remove test as a working second streamed screen.

Microsoft documents the [IddCx driver model](https://learn.microsoft.com/en-us/windows-hardware/drivers/display/indirect-display-driver-model-overview). A production driver also needs Windows Driver Kit build steps and signing. Development signing and production signing must be documented separately when a driver exists. Quest Display will require explicit elevation and consent before any future driver installation.
