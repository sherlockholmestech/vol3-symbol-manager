# vol3-symbol-manager (`vol3sm`)

A fast, CLI-based symbol manager for [Volatility 3](https://github.com/volatilityfoundation/volatility3). 

`vol3sm` allows you to effortlessly search for and install Intermediate Symbol Format (ISF) files directly from public community repositories, saving you the hassle of manually downloading and placing JSON files in the correct directories.

## Features

- **Fast, Fuzzy Searching**: Quickly find symbols by kernel banner, distro name, kernel version, architecture, or filename snippet.
- **Automated Installation**: Downloads and correctly places `.json` or `.json.xz` symbol files into your local Volatility 3 symbols directory.
- **Multiple Sources**: Aggregates symbols from popular community-maintained repositories.
- **Local Management**: List your locally installed symbols and instantly locate your Volatility 3 symbols directory.

## Installation

Ensure you have [Rust and Cargo installed](https://rustup.rs/). Then, clone the repository and install it:

```bash
git clone https://github.com/yourusername/vol3-symbol-manager.git
cd vol3-symbol-manager
cargo install --path .
```

This will place the `vol3sm` binary in your `~/.cargo/bin` directory.

## Usage

You can view the help menu at any time by running:
```bash
vol3sm --help
```

### 1. Search for Symbols
Search using snippets of a banner, kernel version, or OS name. The search utilizes a fuzzy-matching and ranking algorithm to bring the most relevant results to the top.

```bash
vol3sm search "ubuntu 6.2.0 aws"
```
*Output will display a tree of matched symbols and assign an `[ID]` to each file.*

### 2. Install a Symbol
You can install a symbol easily using the `[ID]` provided by your last search, or by providing the exact kernel banner.

**By ID (from previous search):**
```bash
vol3sm install 0
```

**By Source Path:**
```bash
vol3sm install "abyss:Ubuntu/amd64/6.2.0/1007/aws/Ubuntu_6.2.0-1007-aws_amd64.json.xz"
```

**By Exact Banner:**
*(Tip: get the banner from your memory dump by running `vol.py -r pretty -f <dump> banners`)*
```bash
vol3sm install --banner "Linux version 6.2.0-1007-aws (buildd@lcy02-amd64-077) ..."
```

### 3. List Local Symbols
View all the symbol files currently installed in your Volatility 3 symbols directory:

```bash
vol3sm local
```

### 4. Locate Symbols Directory
Print the absolute path to where Volatility 3 is configured to look for symbols on your system:

```bash
vol3sm symbols-dir
```

## Supported Symbol Indexes

Currently, `vol3sm` aggregates and fetches symbols from the following community repositories:
- [Abyss-W4tcher/volatility3-symbols](https://github.com/Abyss-W4tcher/volatility3-symbols)
- [leludo84/vol3-linux-profiles](https://github.com/leludo84/vol3-linux-profiles)
- [p0dalirius/volatility3-symbols](https://github.com/p0dalirius/volatility3-symbols)
