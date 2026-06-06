# AUR packaging

Files for publishing `tg-media-bot` to the [AUR](https://aur.archlinux.org/).
The package installs the bot to `/usr/lib/tg-media-bot`, a `tg-media-bot`
launcher on `PATH`, a systemd unit, and a sample config at
`/etc/tg-media-bot/.env`.

## Install (once it's on the AUR)

```bash
# with an AUR helper
paru -S tg-media-bot      # or: yay -S tg-media-bot

# then configure and start
sudoedit /etc/tg-media-bot/.env      # set BOT_TOKEN, ALLOWED_USERS
sudo systemctl enable --now tg-media-bot
journalctl -u tg-media-bot -f
```

> The systemd unit runs under `DynamicUser`, so browser-cookie auth isn't
> available — set `USE_BROWSER_COOKIES=false` and, if you need authenticated
> downloads, point `COOKIES_FILE` at a readable `cookies.txt`.
>
> For uploads larger than 50 MB, also install `telegram-bot-api` and set
> `API_SERVER_URL` (see the main README).

## Build / test locally before publishing

```bash
cd packaging/aur
makepkg -si            # build + install
namcap PKGBUILD        # lint the PKGBUILD
namcap tg-media-bot-*.pkg.tar.zst
```

## Publish to the AUR

```bash
# 1. Clone the (empty) AUR repo — needs your AUR account + SSH key
git clone ssh://aur@aur.archlinux.org/tg-media-bot.git aur-tg-media-bot
cd aur-tg-media-bot

# 2. Copy the package files in
cp ../tg-media-bot/packaging/aur/{PKGBUILD,.SRCINFO,tg-media-bot.service,tg-media-bot.sh} .

# 3. Regenerate .SRCINFO if you changed PKGBUILD, then push
makepkg --printsrcinfo > .SRCINFO
git add PKGBUILD .SRCINFO tg-media-bot.service tg-media-bot.sh
git commit -m "Initial import: tg-media-bot 0.1.0"
git push
```

## Bumping the version

1. Update `pkgver` (and reset `pkgrel=1`) in `PKGBUILD`.
2. Refresh the checksum: `updpkgsums`.
3. `makepkg --printsrcinfo > .SRCINFO`.
4. Commit and push to the AUR repo.
