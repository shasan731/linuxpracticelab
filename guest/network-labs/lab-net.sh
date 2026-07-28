#!/usr/bin/env bash
#
# Builds the internal laboratory network from Linux network namespaces.
#
# The whole topology lives inside the guest. There is no host interface, no bridge to the
# learner's LAN and no NAT: `ip`, `ping`, `ss`, `dig`, `curl` and `nft` all behave exactly as
# they would on a real network, and the results are deterministic because nothing outside the
# VM can influence them. That determinism is why the specification prefers namespaces over
# QEMU user-mode networking for routing and ping lessons.
#
#   student 10.20.0.10 ─┐
#   web1    10.20.0.21 ─┤
#   web2    10.20.0.22 ─┼── br-lab (10.20.0.0/24) ── router1 ── br-wan (10.30.0.0/24) ── db1
#   dns1    10.20.0.53 ─┘                10.20.0.1 / 10.30.0.1              10.30.0.31
#   attacker 10.20.0.66 ┘
#
# Usage: lab-net up [namespaces...] | down | status

set -Eeuo pipefail

readonly LAB_BRIDGE=br-lab
readonly WAN_BRIDGE=br-wan
readonly LAB_NET=10.20.0
readonly WAN_NET=10.30.0
readonly STATE_DIR=/run/linuxlab/netns

log()  { printf '==> %s\n' "$*"; }
warn() { printf 'warn: %s\n' "$*" >&2; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }

# Address and role of each namespace. Kept in one place so a lesson referring to web1.lab and
# the DNS zone file cannot disagree.
namespace_address() {
    case "$1" in
        student)  echo "${LAB_NET}.10/24" ;;
        web1)     echo "${LAB_NET}.21/24" ;;
        web2)     echo "${LAB_NET}.22/24" ;;
        dns1)     echo "${LAB_NET}.53/24" ;;
        attacker) echo "${LAB_NET}.66/24" ;;
        router1)  echo "${LAB_NET}.1/24" ;;
        db1)      echo "${WAN_NET}.31/24" ;;
        *)        return 1 ;;
    esac
}

namespace_bridge() {
    case "$1" in
        db1) echo "${WAN_BRIDGE}" ;;
        *)   echo "${LAB_BRIDGE}" ;;
    esac
}

require_root() {
    [[ "${EUID}" -eq 0 ]] || die "lab-net needs root; run it with sudo"
}

ensure_bridge() {
    local bridge="$1" address="$2"
    if ! ip link show "${bridge}" >/dev/null 2>&1; then
        ip link add name "${bridge}" type bridge
        ip address add "${address}" dev "${bridge}"
        ip link set "${bridge}" up
    fi
}

# veth names are limited to 15 characters, so the namespace name is truncated rather than
# assumed to fit. Two namespaces sharing a truncated prefix would silently collide.
veth_host_name() {
    printf 'v-%.12s' "$1"
}

create_namespace() {
    local name="$1"
    local address bridge host_side
    address="$(namespace_address "${name}")" || die "unknown namespace '${name}'"
    bridge="$(namespace_bridge "${name}")"
    host_side="$(veth_host_name "${name}")"

    if ip netns list | grep -qw "${name}"; then
        log "namespace ${name} already exists"
        return 0
    fi

    log "creating namespace ${name} (${address})"
    ip netns add "${name}"

    ip link add "${host_side}" type veth peer name eth0 netns "${name}"
    ip link set "${host_side}" master "${bridge}"
    ip link set "${host_side}" up

    ip -n "${name}" address add "${address}" dev eth0
    ip -n "${name}" link set eth0 up
    # Loopback in a fresh namespace starts down, which breaks anything binding to 127.0.0.1.
    ip -n "${name}" link set lo up

    # Everything except the router points at the router for anything off its own subnet. The
    # routing lessons work by removing this and asking the learner to put it back.
    if [[ "${name}" != "router1" ]]; then
        local gateway
        case "${bridge}" in
            "${WAN_BRIDGE}") gateway="${WAN_NET}.1" ;;
            *)               gateway="${LAB_NET}.1" ;;
        esac
        ip -n "${name}" route add default via "${gateway}" 2>/dev/null || \
            warn "could not add a default route in ${name}"
    fi

    install -d -m 0755 "/etc/netns/${name}"
    printf 'nameserver %s.53\nsearch lab\n' "${LAB_NET}" > "/etc/netns/${name}/resolv.conf"

    mkdir -p "${STATE_DIR}"
    printf '%s\n' "${address}" > "${STATE_DIR}/${name}"
}

configure_router() {
    ip netns list | grep -qw router1 || return 0
    log "configuring router1"
    # The router needs a leg on both subnets and forwarding enabled; without the second leg the
    # 10.30.0.0/24 side is unreachable and the troubleshooting lesson has a real fault to find.
    if ! ip -n router1 link show eth1 >/dev/null 2>&1; then
        ip link add v-router1-w type veth peer name eth1 netns router1
        ip link set v-router1-w master "${WAN_BRIDGE}"
        ip link set v-router1-w up
        ip -n router1 address add "${WAN_NET}.1/24" dev eth1
        ip -n router1 link set eth1 up
    fi
    ip netns exec router1 sysctl -q -w net.ipv4.ip_forward=1
}

start_services() {
    # Local DNS for the .lab zone, so dig and nslookup have something real to answer them.
    if ip netns list | grep -qw dns1 && command -v dnsmasq >/dev/null 2>&1; then
        log "starting DNS in dns1"
        ip netns exec dns1 dnsmasq \
            --no-daemon \
            --pid-file=/run/linuxlab/dns1.pid \
            --listen-address="${LAB_NET}.53" \
            --bind-interfaces \
            --no-resolv \
            --no-hosts \
            --addn-hosts=/opt/linuxlab/network-labs/lab-hosts \
            --local=/lab/ \
            --domain=lab \
            >/dev/null 2>&1 &
    fi

    # Two web servers so virtual-host and load-balancing lessons have somewhere to point.
    for web in web1 web2; do
        ip netns list | grep -qw "${web}" || continue
        local docroot="/srv/${web}"
        install -d -m 0755 "${docroot}"
        if [[ ! -f "${docroot}/index.html" ]]; then
            printf '<h1>%s</h1><p>Internal laboratory web server.</p>\n' "${web}" > "${docroot}/index.html"
        fi
        # python3 -m http.server is enough for the HTTP lessons and needs no configuration
        # file, which keeps the nginx lessons free to be about nginx.
        log "starting HTTP in ${web}"
        ip netns exec "${web}" python3 -m http.server 80 --directory "${docroot}" \
            --bind "$(namespace_address "${web}" | cut -d/ -f1)" >/dev/null 2>&1 &
    done
}

bring_up() {
    require_root
    ensure_bridge "${LAB_BRIDGE}" "${LAB_NET}.254/24"
    ensure_bridge "${WAN_BRIDGE}" "${WAN_NET}.254/24"

    local requested=("$@")
    if (( ${#requested[@]} == 0 )); then
        requested=(student web1 web2 dns1 router1 db1)
    fi

    # router1 first: everything else adds a default route pointing at it.
    for name in router1 "${requested[@]}"; do
        case " ${requested[*]} router1 " in
            *" ${name} "*) create_namespace "${name}" ;;
        esac
    done
    configure_router
    start_services
    log "internal laboratory network is up"
}

bring_down() {
    require_root
    log "tearing down the internal laboratory network"
    for pidfile in /run/linuxlab/*.pid; do
        [[ -f "${pidfile}" ]] || continue
        kill "$(cat "${pidfile}")" 2>/dev/null || true
        rm -f "${pidfile}"
    done
    while read -r name _; do
        [[ -n "${name}" ]] || continue
        ip netns delete "${name}" 2>/dev/null || true
        rm -rf "/etc/netns/${name}"
    done < <(ip netns list)
    for bridge in "${LAB_BRIDGE}" "${WAN_BRIDGE}"; do
        ip link delete "${bridge}" 2>/dev/null || true
    done
    rm -rf "${STATE_DIR}"
}

status() {
    printf 'namespaces:\n'
    ip netns list || true
    printf '\nbridges:\n'
    ip -brief link show type bridge || true
}

case "${1:-status}" in
    up)     shift; bring_up "$@" ;;
    down)   bring_down ;;
    status) status ;;
    *)      die "usage: lab-net up [namespaces...] | down | status" ;;
esac
