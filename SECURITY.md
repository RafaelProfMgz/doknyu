# Security policy

## Supported versions

Only the latest release receives security fixes.

## Reporting a vulnerability

Please **do not open a public issue**. Use one of the private channels:

- GitHub's private reporting: [Report a vulnerability](https://github.com/RafaelProfMgz/doknyu/security/advisories/new)
  (preferred), or
- e-mail <rafael@studiomega.com.br>.

Include what you found, how to reproduce it and the version (`doknyu --version`) and install
method. You should get an answer within 7 days. Once a fix is released, the advisory is published
with credit to you, unless you prefer to stay anonymous.

## Scope

doknyu talks to the local Docker daemon through its socket. Access to that socket is equivalent to
root on the machine; this is a property of Docker, not a vulnerability in doknyu. Reports about how
doknyu handles data from the Docker API, the embedded terminal, the packages (`.deb`, AppImage,
Flatpak, AUR, RPM, Nix) or the release process are in scope.
