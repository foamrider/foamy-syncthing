# Foamy Syncthing

Syncthing folder status and controls.

Status colors adapt to light and dark themes. Popup corners use twice Hyprland's
`decoration:rounding` value through Omarchy; zero keeps them square.

![Foamy Syncthing screenshot](preview.png)

## Install

Requires Omarchy Quattro on **x86-64 Linux**, Bash, `jq`, GNU coreutils,
and systemd user services. The repository includes the compiled
`bin/x86_64/syncshell-core` helper and Web UI assets. This helper connects the
plugin to Syncthing; **Syncthing itself is not bundled**. Other CPU architectures
are not supported by the bundled helper.

Install Syncthing (`syncthing`) and configure its user service. Disable any
other Syncthing shell plugin before enabling this one.

If Syncthing is missing, the plugin's install action opens an Omarchy terminal
and runs `omarchy pkg add syncthing`, followed by
`systemctl --user enable --now syncthing.service`. Package installation uses
Omarchy's normal authorization flow. This setup is started by the user;
adding the plugin alone does not install the Syncthing package.

```sh
omarchy plugin add https://github.com/foamrider/foamy-syncthing.git --enable
```

## Use

- Left-click the widget, then use the Folders and Devices tabs. Expand a row for
  shared folders/devices, status, and error details. Right-click refreshes.
- Device syncing/error labels reflect local activity in shared folders; they do
  not confirm a transfer to that specific device. Open folders from their details.
- Open the cog for language, device warnings, and folder actions.
- Use the globe to open Syncthing's Web UI.
- The footer play/pause button starts or stops the local Syncthing service.

**Pause** keeps a folder configured. **Unlink** asks for confirmation and removes
it from Syncthing while keeping local files. **Add folder** uses an existing
directory.

Web UI setup and theme refresh only replace an installation whose file contents,
paths, types, and permissions still match the recorded installation fingerprint.
Added or edited files, missing files, extra directories, and symbolic links cause
the whole theme directory to be preserved. Older installations with only an
ownership marker or no marker are also preserved. If setup reports an unowned
or modified path, move that directory aside manually before retrying.

## Remove

```sh
omarchy plugin remove foamy.syncthing
```

This removes the shell plugin and stops its Syncshell helper. It does not
uninstall Syncthing, stop its independent user service, delete synchronized
files, or undo folder/device changes already made in Syncthing. Plugin settings,
state, and installed Web UI themes remain on disk with this command. To stop
synchronization as well, stop the Syncthing user service separately before
removing the plugin. The panel's separate removal dialog offers plugin-data
cleanup; review its confirmation before choosing that option.
That cleanup deletes only Web UI theme directories that still match their
installation fingerprint. Modified, unmarked, and legacy marked directories
are preserved in full.

Omarchy manages the plugin entry in `shell.json`. Packages and data outside
the plugin directory are retained unless you remove them separately.

## License

Plugin code is [MIT-licensed](LICENSE), based on [Syncshell](LICENSE-SYNCSHELL)
and [Omarchy](LICENSE-OMARCHY). Bundled assets retain their own license notices.

Provided **as is**, without warranty or guaranteed support. Use at your own risk.
