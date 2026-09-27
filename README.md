# Foamy Syncthing

Syncthing folder status and controls.

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

- Left-click the widget to see folders and connected devices; right-click refreshes.
- Open the cog for language, device warnings, and folder actions.
- Use the globe to open Syncthing's Web UI.
- The footer play/pause button starts or stops the local Syncthing service.

**Pause** keeps a folder configured. **Unlink** asks for confirmation and removes
it from Syncthing while keeping local files. **Add folder** uses an existing
directory.

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

Omarchy manages the plugin entry in `shell.json`. Packages and data outside
the plugin directory are retained unless you remove them separately.

## License

Plugin code is [MIT-licensed](LICENSE), based on [Syncshell](LICENSE-SYNCSHELL)
and [Omarchy](LICENSE-OMARCHY). Bundled assets retain their own license notices.

Provided **as is**, without warranty or guaranteed support. Use at your own risk.
