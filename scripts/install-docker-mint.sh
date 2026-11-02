#!/usr/bin/env bash
# Run interactively in your own terminal; never send a password to the assistant.
set -euo pipefail
. /etc/os-release
if [[ "${ID:-}" != "linuxmint" || "${UBUNTU_CODENAME:-}" != "noble" ]]; then
  echo "Este procedimento foi preparado para Linux Mint baseado no Ubuntu noble."
  exit 1
fi
if [[ -e /etc/apt/sources.list.d/docker.sources || -e /etc/apt/sources.list.d/docker.list ]]; then
  echo "Já existe configuração de repositório Docker; revise antes de continuar."
  exit 1
fi
for package in docker.io docker-compose docker-compose-v2 docker-doc docker-buildx podman-docker containerd runc; do
  if dpkg-query -W -f='${Status}' "$package" 2>/dev/null | grep -qx 'install ok installed'; then
    echo "Pacote conflitante instalado: $package. Consulte docs/setup-docker.md. Nada foi removido."
    exit 1
  fi
done
sudo -v
sudo apt-get update
sudo apt-get install ca-certificates curl
sudo install -m 0755 -d /etc/apt/keyrings
sudo curl -fsSL https://download.docker.com/linux/ubuntu/gpg -o /etc/apt/keyrings/docker.asc
sudo chmod a+r /etc/apt/keyrings/docker.asc
forge_arch=$(dpkg --print-architecture)
sudo tee /etc/apt/sources.list.d/docker.sources >/dev/null <<EOF
Types: deb
URIs: https://download.docker.com/linux/ubuntu
Suites: noble
Components: stable
Architectures: $forge_arch
Signed-By: /etc/apt/keyrings/docker.asc
EOF
sudo apt-get update
sudo apt-get install docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
sudo systemctl start docker
sudo docker run --rm hello-world
sudo docker compose version
echo "Docker instalado e teste executado. Consulte docs/setup-docker.md para permissões."
