# fini for VS Code

Automatically normalize file formatting on save using [fini](https://github.com/tsukasaI/fini).

## Features

- Normalize whitespace, line endings, and trailing newlines on save
- Manual format command: `fini: Format Document with fini`
- Configurable fini executable path and arguments

## Requirements

Install fini:

```bash
cargo install fini
# or
brew tap tsukasaI/fini https://github.com/tsukasaI/fini
brew install tsukasaI/fini/fini
```

## Extension Settings

- `fini.enable`: Enable/disable fini (default: true)
- `fini.formatOnSave`: Run fini when saving files (default: true)
- `fini.path`: Path to fini executable (default: "fini")
- `fini.args`: Additional arguments to pass to fini

## Usage

Once installed and fini is in your PATH, files will be automatically normalized on save.
