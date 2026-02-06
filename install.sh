#!/bin/sh
set -eu

REPO="lucasilverentand/project-dash"
BINARY="project-dash"

get_target() {
    os=$(uname -s)
    arch=$(uname -m)

    case "$os" in
        Linux)
            case "$arch" in
                x86_64)  echo "x86_64-unknown-linux-musl" ;;
                aarch64) echo "aarch64-unknown-linux-gnu" ;;
                *)       echo "Unsupported architecture: $arch" >&2; exit 1 ;;
            esac
            ;;
        Darwin)
            case "$arch" in
                x86_64)  echo "x86_64-apple-darwin" ;;
                arm64)   echo "aarch64-apple-darwin" ;;
                *)       echo "Unsupported architecture: $arch" >&2; exit 1 ;;
            esac
            ;;
        *)
            echo "Unsupported OS: $os" >&2
            exit 1
            ;;
    esac
}

get_latest_tag() {
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
        | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p'
}

main() {
    target=$(get_target)
    tag=$(get_latest_tag)

    if [ -z "$tag" ]; then
        echo "Error: could not determine latest release tag." >&2
        exit 1
    fi

    asset="${BINARY}-${target}.tar.gz"
    url="https://github.com/${REPO}/releases/download/${tag}/${asset}"

    echo "Downloading ${BINARY} ${tag} for ${target}..."

    tmpdir=$(mktemp -d)
    trap 'rm -rf "$tmpdir"' EXIT

    curl -fsSL "$url" -o "${tmpdir}/${asset}"
    tar xzf "${tmpdir}/${asset}" -C "$tmpdir"

    install_dir="${HOME}/.local/bin"
    mkdir -p "$install_dir"
    mv "${tmpdir}/${BINARY}" "${install_dir}/${BINARY}"
    chmod +x "${install_dir}/${BINARY}"

    echo ""
    echo "${BINARY} ${tag} installed to ${install_dir}/${BINARY}"

    case ":${PATH}:" in
        *":${install_dir}:"*) ;;
        *)
            echo ""
            echo "Add ${install_dir} to your PATH:"
            echo "  export PATH=\"${install_dir}:\$PATH\""
            ;;
    esac
}

main
