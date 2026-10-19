# Installing a test build on Ubuntu

Every change pushed to GitHub builds a fresh installable package automatically.

## Download

1. Open https://github.com/prophetizer/lanternpost/actions and sign in.
2. Click the newest run with a green tick.
3. Scroll to **Artifacts** at the bottom and download **lanternpost-deb**.
   **screenshots** shows what that build looks like, if you want to check before installing.
4. The download is a `.zip`. Double-click it to extract `lanternpost_….deb`.

## Install

Open a terminal in the folder with the `.deb` and run:

```
sudo apt install ./lanternpost_*.deb
```

`apt` also installs the Qt libraries LanternPost needs. Then open **LanternPost** from the app grid.
To update, download a newer build and run the same command.

## Remove

```
sudo apt remove lanternpost
```

## Reporting a problem

Say what you did, what you expected and what happened. A screenshot helps. If the app won't
start, run `lanternpost` in a terminal and copy what it prints.
