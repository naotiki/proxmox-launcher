# Proxmox Host Desktop VM Launcher 仕様書

## 1. 概要

本アプリケーションは、Proxmox VEホスト上で動作するTUIアプリケーションである。

Proxmoxホストにインストールされたデスクトップ環境上で実行し、ホスト上の仮想マシンを一覧表示し、起動・停止・シャットダウン等の電源操作、およびVNC/SPICEによるリモートデスクトップビューアー起動を行う。

Proxmoxに対する操作は原則としてCLI経由で実行し、APIトークンやAPIユーザーの管理を不要にする。

## 2. 目的

### 2.1 背景

Proxmox VE標準のWeb UIでは、VMコンソールにnoVNCまたはSPICEを利用できるが、ホストに直接接続されたモニターや、ホストへRDP/VNC接続したデスクトップ環境上でVMを素早く切り替えて操作したい場合、Web UI経由の操作はやや煩雑である。

また、各VMにRDPやVNCサーバーを個別に導入する方式では、ゲストOSごとの設定が必要となる。

本アプリケーションは、VM側に追加設定を要求せず、Proxmoxホスト上の既存機能を利用してVMコンソールへ接続することを目的とする。

### 2.2 達成したいこと

- Proxmoxホスト上のVMをTUIで一覧表示する
- VMの状態を確認できる
- VMを起動、シャットダウン、停止、再起動できる
- 選択したVMに対してVNCまたはSPICEで接続できる
- VNC接続時はProxmoxのVNC proxyを起動し、Remminaで開く
- SPICE接続時はProxmoxから`.vv`ファイルを取得し、virt-viewerで開く
- Proxmox APIトークンを保持しない
- 原則としてroot権限またはsudo経由で`qm`/`pvesh`を実行する

## 3. 想定実行環境

### 3.1 実行場所

本アプリケーションはProxmox VEホスト上で実行する。

```text
Proxmox VE Host
├── Proxmox VE
├── Desktop Environment
│   ├── TUI App
│   ├── Remmina
│   └── virt-viewer / remote-viewer
└── VMs
```

### 3.2 想定OS

- Proxmox VE 8.x以降
- DebianベースのProxmox VEホスト
- ホスト上に軽量GUI環境が導入されていること

例:

- XFCE
- LXQt
- Fluxbox
- Openbox

### 3.3 必須コマンド

アプリケーションは以下のコマンドが利用可能であることを前提とする。

| コマンド | 用途 |
|---|---|
| `qm` | VM一覧取得、VM電源操作、VNC proxy操作 |
| `pvesh` | Proxmox内部API呼び出し |
| `remmina` | VNCビューアー |
| `remote-viewer` | SPICE `.vv` ファイルを開く |
| `mktemp` | 一時ファイル作成 |
| `jq` | JSONパース |
| `setsid` または `nohup` | ビューアーの非同期起動 |

### 3.4 推奨パッケージ

```bash
apt update
apt install -y jq remmina virt-viewer
```

必要に応じてGUI環境も導入する。

```bash
apt install -y --no-install-recommends xfce4 lightdm
```

## 4. スコープ

### 4.1 対象範囲

本アプリケーションは以下を対象とする。

- QEMU/KVM VM
- Proxmoxホスト上で実行中または停止中のVM
- VMの基本操作
- VNC接続
- SPICE接続
- ローカルホスト上のビューアー起動

### 4.2 対象外

以下は初期実装では対象外とする。

- LXCコンテナへの接続
- Proxmoxクラスタ全体の高度な管理
- VM作成・削除
- ストレージ操作
- スナップショット管理
- バックアップ管理
- VM設定変更
- APIトークン管理
- Webブラウザ版noVNCの再実装
- ユーザー管理
- RBAC管理
- 複数ユーザー同時利用

## 5. 基本方針

### 5.1 Proxmox操作方針

Proxmoxへの操作はCLI経由で行う。

優先順位は以下とする。

1. `qm`で直接実行できる操作は`qm`を使う
2. `qm`で扱いにくい操作は`pvesh`を使う
3. HTTP APIを直接叩く処理は原則使わない
4. APIトークン、PVEAuthCookie、CSRFPreventionTokenはアプリケーション内で管理しない

### 5.2 権限方針

本アプリケーションはProxmoxホスト上で管理者が使う前提とする。

実行方式は以下のいずれかとする。

- rootで実行
- sudo経由で必要なコマンドを実行
- systemd-run等で特権ヘルパーを呼び出す

初期実装ではroot実行を前提とする。

```bash
sudo pve-vm-launcher
```

### 5.3 接続方式方針

#### VNC

ProxmoxのVNC proxyを起動し、Remminaで接続する。

ただし、ProxmoxのVNC proxyは通常の常時公開VNCサーバーではなく、一時的なプロキシ接続である。そのため、以下の制約を持つ。

- 接続開始から一定時間内にVNCクライアントが接続する必要がある
- 接続情報は都度生成する
- Remmina側がProxmoxのVNC proxy仕様に対応できない場合がある
- 実装ではVNC接続を「試験的機能」として扱う

#### SPICE

`pvesh create /nodes/{node}/qemu/{vmid}/spiceproxy` によりSPICE接続用の`.vv`ファイルを生成し、`remote-viewer`で開く。

SPICEはVNCよりもリッチなコンソール体験を提供するため、対応VMではSPICEを推奨接続方式とする。

## 6. UI仕様

### 6.1 画面構成

TUIは以下の画面を持つ。

1. VM一覧画面
2. VM操作メニュー
3. 接続方式選択画面
4. 確認ダイアログ
5. エラー表示画面
6. ログ表示画面

### 6.2 VM一覧画面

起動時にVM一覧画面を表示する。

表示項目:

| 項目 | 内容 |
|---|---|
| VMID | VM ID |
| Name | VM名 |
| Status | `running` / `stopped` / `paused` 等 |
| Node | 実行ノード名 |
| Display | 推定接続方式 |
| Uptime | 実行中の場合の稼働時間 |
| Memory | 使用メモリまたは割当メモリ |
| CPU | CPU使用率またはvCPU数 |

最小表示は以下とする。

```text
┌──────┬────────────────────────┬─────────┬────────┐
│ VMID │ Name                   │ Status  │ Display│
├──────┼────────────────────────┼─────────┼────────┤
│ 100  │ win11-dev              │ running │ spice  │
│ 101  │ ubuntu-lab             │ stopped │ vnc    │
│ 102  │ kali-test              │ running │ vnc    │
└──────┴────────────────────────┴─────────┴────────┘
```

### 6.3 キーバインド

| キー | 操作 |
|---|---|
| `↑` / `k` | 上へ移動 |
| `↓` / `j` | 下へ移動 |
| `Enter` | VM操作メニューを開く |
| `r` | 一覧更新 |
| `s` | 選択VMを起動 |
| `S` | 選択VMをシャットダウン |
| `f` | 選択VMを強制停止 |
| `b` | 選択VMを再起動 |
| `v` | VNCで接続 |
| `p` | SPICEで接続 |
| `l` | ログ表示 |
| `q` | 終了 |

### 6.4 VM操作メニュー

VMを選択して`Enter`を押すと操作メニューを表示する。

```text
VM: 100 win11-dev [running]

[1] Attach via VNC
[2] Attach via SPICE
[3] Start
[4] Shutdown
[5] Reboot
[6] Stop / Power Off
[7] Reset
[8] Show config
[9] Back
```

### 6.5 確認ダイアログ

破壊的操作には確認を挟む。

対象操作:

- Stop
- Reset
- Shutdown
- Reboot

例:

```text
VM 100 win11-dev をシャットダウンしますか？

[y] Yes
[n] No
```

## 7. VM一覧取得仕様

### 7.1 取得方法

初期実装では`qm list`を利用する。

```bash
qm list
```

出力例:

```text
      VMID NAME                 STATUS     MEM(MB)    BOOTDISK(GB) PID
       100 win11-dev            running    8192       64.00        12345
       101 ubuntu-lab           stopped    4096       32.00        0
```

### 7.2 パース項目

`qm list`から以下を取得する。

| 内部名 | 取得元 |
|---|---|
| `vmid` | VMID |
| `name` | NAME |
| `status` | STATUS |
| `memory_mb` | MEM(MB) |
| `bootdisk_gb` | BOOTDISK(GB) |
| `pid` | PID |

### 7.3 代替取得方式

より正確な取得が必要な場合、`pvesh`を利用する。

```bash
pvesh get /nodes/{node}/qemu --output-format json
```

`node`は以下で取得する。

```bash
hostname
```

または

```bash
pvesh get /nodes --output-format json
```

### 7.4 VM状態の詳細取得

選択VMについて詳細状態を取得する場合は以下を使う。

```bash
pvesh get /nodes/{node}/qemu/{vmid}/status/current --output-format json
```

取得する項目:

- `status`
- `name`
- `uptime`
- `cpu`
- `cpus`
- `mem`
- `maxmem`
- `disk`
- `maxdisk`
- `pid`

## 8. VM操作仕様

### 8.1 起動

```bash
qm start {vmid}
```

または

```bash
pvesh create /nodes/{node}/qemu/{vmid}/status/start
```

### 8.2 シャットダウン

```bash
qm shutdown {vmid}
```

または

```bash
pvesh create /nodes/{node}/qemu/{vmid}/status/shutdown
```

### 8.3 強制停止

```bash
qm stop {vmid}
```

または

```bash
pvesh create /nodes/{node}/qemu/{vmid}/status/stop
```

### 8.4 再起動

通常再起動:

```bash
qm reboot {vmid}
```

代替方式:

```bash
pvesh create /nodes/{node}/qemu/{vmid}/status/reboot
```

### 8.5 リセット

```bash
qm reset {vmid}
```

または

```bash
pvesh create /nodes/{node}/qemu/{vmid}/status/reset
```

### 8.6 操作後の状態更新

電源操作実行後はVM一覧を再取得する。

更新タイミング:

- 操作直後
- 1秒後
- 3秒後
- 5秒後

最大5秒までポーリングし、その時点の状態を表示する。

## 9. VNC接続仕様

### 9.1 基本方針

VNC接続はProxmoxのVNC proxyを利用する。

接続フロー:

```text
TUI App
  ↓
pvesh create /nodes/{node}/qemu/{vmid}/vncproxy
  ↓
一時VNC接続情報を取得
  ↓
Remmina用プロファイル生成
  ↓
remmina起動
  ↓
RemminaがVNC proxyへ接続
```

### 9.2 VNC proxy起動

以下を実行する。

```bash
pvesh create /nodes/{node}/qemu/{vmid}/vncproxy --output-format json
```

必要に応じて以下のパラメータを付与する。

```bash
pvesh create /nodes/{node}/qemu/{vmid}/vncproxy \
  --websocket 0 \
  --output-format json
```

### 9.3 取得する情報

戻り値から以下を取得する。

| 項目 | 用途 |
|---|---|
| `port` | Remmina接続先ポート |
| `ticket` | VNC認証用パスワード相当 |
| `cert` | TLS証明書情報 |
| `upid` | ProxmoxタスクID |
| `user` | 接続ユーザー |

### 9.4 Remminaプロファイル生成

一時ディレクトリにRemminaプロファイルを生成する。

```text
/tmp/pve-vm-launcher/remmina-{vmid}.remmina
```

プロファイル例:

```ini
[remmina]
name=Proxmox VM {vmid}
protocol=VNC
server=127.0.0.1:{port}
password={ticket}
disableclipboard=0
viewmode=1
quality=9
colordepth=32
```

### 9.5 Remmina起動

```bash
setsid remmina -c /tmp/pve-vm-launcher/remmina-{vmid}.remmina >/dev/null 2>&1 &
```

### 9.6 VNC接続の注意事項

ProxmoxのVNC proxyは通常の常時公開VNCサーバーではない。

そのため、以下の制約を仕様として明記する。

- `vncproxy`起動後、短時間以内にRemminaを接続する必要がある
- RemminaのVNC/TLS対応状況によって接続できない場合がある
- 接続失敗時はSPICE接続を案内する
- VNC接続は初期バージョンではexperimental扱いとする
- RemminaがProxmoxのVNC proxyと相性が悪い場合、TigerVNC Viewer等への差し替えを可能にする

### 9.7 `qm vncproxy`を使うフォールバック

`pvesh create .../vncproxy`が利用できない場合、`qm vncproxy {vmid}`を用いる。

ただし、`qm vncproxy`はstdin/stdoutにVNC trafficを流すため、Remminaへ直接渡せない。

その場合は以下のようなローカルTCPラッパーを用意する。

```text
Remmina
  ↓ TCP
localhost:{local_port}
  ↓
socat
  ↓ stdin/stdout
qm vncproxy {vmid}
```

概念コマンド:

```bash
socat TCP-LISTEN:{local_port},fork EXEC:"qm vncproxy {vmid}"
```

この方式は実装が複雑になるため、初期実装では優先度を下げる。

## 10. SPICE接続仕様

### 10.1 基本方針

SPICE接続は、Proxmox内部APIから`.vv`ファイルを生成し、`remote-viewer`で開く。

接続フロー:

```text
TUI App
  ↓
pvesh create /nodes/{node}/qemu/{vmid}/spiceproxy
  ↓
.vvファイル生成
  ↓
remote-viewerで開く
```

### 10.2 SPICE proxy情報取得

`pvesh`の`text`出力は表形式であり、そのまま`.vv`ファイルとしては使えない。
そのため、初期実装ではJSONとして取得し、アプリ側で`[virt-viewer]`形式の`.vv`ファイルへ変換する。

```bash
pvesh create /nodes/{node}/qemu/{vmid}/spiceproxy --output-format json
```

生成する`.vv`ファイル例:

```ini
[virt-viewer]
type=spice
host=pvespiceproxy:...
tls-port=61000
password=...
proxy=http://basic.home.lan:3128
ca=-----BEGIN CERTIFICATE-----\n...
host-subject=OU=PVE Cluster Node,O=Proxmox Virtual Environment,CN=basic.home.lan
```

### 10.3 virt-viewer起動

```bash
setsid remote-viewer /tmp/pve-vm-launcher/spice-{vmid}.vv >/dev/null 2>&1 &
```

### 10.4 `.vv`ファイルの扱い

`.vv`ファイルは一時ファイルとして扱う。

保存場所:

```text
/tmp/pve-vm-launcher/spice-{vmid}-{timestamp}.vv
```

削除タイミング:

- `remote-viewer`起動から数秒後
- アプリ終了時
- 次回起動時のクリーンアップ処理

ただし、`remote-viewer`がファイルを読み込む前に削除されないよう、削除は遅延実行する。

例:

```bash
( sleep 30; rm -f /tmp/pve-vm-launcher/spice-{vmid}-{timestamp}.vv ) &
```

### 10.5 SPICE対応判定

VMがSPICEに対応しているかを確認するため、VM設定を取得する。

```bash
qm config {vmid}
```

確認項目:

- `vga: qxl`
- `vga: virtio`
- `spice_enhancements`
- `agent`

初期実装では厳密な判定を行わず、SPICE接続を試行し、失敗した場合にエラーを表示する。

### 10.6 SPICE接続失敗時の扱い

以下の場合はエラーとして表示する。

- `.vv`ファイルが空
- `pvesh create .../spiceproxy`が非0終了
- `remote-viewer`が存在しない
- VMが停止中
- SPICE displayが未設定
- spiceproxyサービスが停止している
- ホスト名解決に失敗する

表示例:

```text
SPICE接続に失敗しました。

考えられる原因:
- VMが起動していない
- VMのDisplayがSPICE/QXLに設定されていない
- remote-viewerがインストールされていない
- spiceproxyサービスが起動していない
```

## 11. 接続方式選択仕様

### 11.1 自動選択

`Attach`操作時は、以下の優先順位で接続方式を決定する。

1. VMがrunningであることを確認
2. VM設定からSPICE対応が推定できればSPICEを優先
3. SPICE非対応または失敗時はVNCを試行
4. 両方失敗した場合はエラー表示

### 11.2 手動選択

ユーザーは明示的にVNCまたはSPICEを選択できる。

```text
接続方式を選択してください。

[1] VNC via Remmina
[2] SPICE via virt-viewer
[3] Auto
[4] Cancel
```

### 11.3 停止中VMへの接続

停止中VMに接続しようとした場合は確認する。

```text
VM 101 ubuntu-lab は停止中です。
起動してから接続しますか？

[y] 起動して接続
[n] キャンセル
```

`y`の場合:

1. `qm start {vmid}`
2. running状態になるまで待機
3. 選択された方式で接続

## 12. 設定ファイル仕様

### 12.1 設定ファイルパス

```text
/etc/pve-vm-launcher/config.toml
```

またはユーザー単位:

```text
~/.config/pve-vm-launcher/config.toml
```

初期実装ではユーザー単位設定を優先する。
初回起動時に設定ファイルが存在しない場合は、デフォルト内容で`~/.config/pve-vm-launcher/config.toml`を作成する。
`sudo`経由で起動された場合は`/root`ではなく`SUDO_USER`のホームディレクトリを優先する。

### 12.2 設定例

```toml
[proxmox]
node = "auto"
command_timeout_sec = 15
prefer_pvesh = true

[viewer]
default_protocol = "auto"

[viewer.spice]
command = "remote-viewer"
args = []
run_as_invoking_user = true

[viewer.spice.env]
# GDK_BACKEND = "x11"

[viewer.vnc]
command = "remmina"
args = []
run_as_invoking_user = true

[viewer.vnc.env]
# GDK_BACKEND = "x11"

[vnc]
enabled = true
experimental = true
connect_timeout_sec = 10
remmina_profile_dir = "/tmp/pve-vm-launcher"

[spice]
enabled = true
vv_dir = "/tmp/pve-vm-launcher"
delete_vv_after_sec = 30

[ui]
refresh_interval_sec = 3
confirm_destructive_actions = true
show_advanced_actions = false

[logging]
level = "info"
file = "~/.local/state/pve-vm-launcher/app.log"
```

### 12.3 デフォルト値

| 設定 | デフォルト |
|---|---|
| `node` | `auto` |
| `default_protocol` | `auto` |
| `viewer.vnc.command` | `remmina` |
| `viewer.vnc.args` | `[]` |
| `viewer.vnc.env` | empty |
| `viewer.vnc.run_as_invoking_user` | `true` |
| `viewer.spice.command` | `remote-viewer` |
| `viewer.spice.args` | `[]` |
| `viewer.spice.env` | empty |
| `viewer.spice.run_as_invoking_user` | `true` |
| `command_timeout_sec` | `15` |
| `confirm_destructive_actions` | `true` |
| `delete_vv_after_sec` | `30` |

## 13. 内部データモデル

### 13.1 VM

```typescript
type Vm = {
  vmid: number
  name: string
  status: "running" | "stopped" | "paused" | "unknown"
  node: string
  memoryMb?: number
  bootdiskGb?: number
  pid?: number
  displayHint?: "vnc" | "spice" | "unknown"
}
```

### 13.2 VM詳細

```typescript
type VmStatus = {
  vmid: number
  name: string
  status: string
  uptime?: number
  cpu?: number
  cpus?: number
  mem?: number
  maxmem?: number
  disk?: number
  maxdisk?: number
  pid?: number
}
```

### 13.3 ViewerSession

```typescript
type ViewerSession = {
  vmid: number
  protocol: "vnc" | "spice"
  startedAt: string
  processId?: number
  tempFiles: string[]
  status: "starting" | "running" | "failed" | "closed"
}
```

### 13.4 CommandResult

```typescript
type CommandResult = {
  command: string[]
  exitCode: number
  stdout: string
  stderr: string
  durationMs: number
}
```

## 14. コマンド実行仕様

### 14.1 実行共通仕様

すべての外部コマンドは以下の条件で実行する。

- shell injectionを避けるため、原則としてargv配列で実行する
- タイムアウトを設定する
- stdout/stderrをログに保存する
- 非0終了時はエラーとして扱う
- ユーザー入力値はVMID等の型チェックを行う

### 14.2 タイムアウト

| 操作 | タイムアウト |
|---|---:|
| VM一覧取得 | 5秒 |
| VM状態取得 | 5秒 |
| 起動 | 30秒 |
| シャットダウン | 60秒 |
| 強制停止 | 30秒 |
| VNC proxy起動 | 10秒 |
| SPICE proxy取得 | 10秒 |
| ビューアー起動 | 5秒 |

### 14.3 ログ出力

ログには以下を記録する。

- 実行時刻
- 操作種別
- VMID
- 実行コマンド
- 終了コード
- stderr
- 処理時間

パス:

```text
~/.local/state/pve-vm-launcher/app.log
```

## 15. エラーハンドリング

### 15.1 コマンド未検出

例:

```text
remmina が見つかりません。
apt install remmina を実行してください。
```

### 15.2 権限不足

例:

```text
Proxmox操作に失敗しました。
このアプリケーションはrootまたはsudo権限で実行してください。
```

### 15.3 VM未起動

例:

```text
VM 101 は停止中です。
起動してから接続してください。
```

### 15.4 VNC接続失敗

例:

```text
VNC接続に失敗しました。

考えられる原因:
- vncproxyの有効期限内にRemminaが接続できなかった
- RemminaがProxmox VNC proxyの認証/TLS方式に対応していない
- VMが停止中
- Proxmox側でコンソール接続を拒否した

SPICE接続を試すことを推奨します。
```

### 15.5 SPICE接続失敗

例:

```text
SPICE接続に失敗しました。

考えられる原因:
- VMのDisplayがSPICEに設定されていない
- remote-viewerがインストールされていない
- spiceproxyサービスが停止している
- .vvファイルの生成に失敗した
```

## 16. セキュリティ仕様

### 16.1 APIトークン非保持

本アプリケーションは以下を保存しない。

- Proxmox APIトークン
- PVEAuthCookie
- CSRFPreventionToken
- Proxmoxログインパスワード

### 16.2 一時ファイル

以下の一時ファイルを生成する。

- Remminaプロファイル
- SPICE `.vv`ファイル
- 一時ログ
- セッション情報

一時ファイルは原則として`0600`で作成する。

```bash
umask 077
```

### 16.3 認証情報を含むファイル

以下は認証情報を含む可能性があるため、適切に削除する。

- `.vv`ファイル
- Remminaプロファイル
- VNC ticketを含む一時ファイル

### 16.4 外部公開禁止

本アプリケーションはローカルホスト上での利用を想定する。

以下は行わない。

- VNC proxyポートの外部公開
- spiceproxyポートの外部公開
- Web APIサーバーの常時公開
- リモートユーザー向けの認証機構提供

## 17. 実装候補

### 17.1 推奨言語

以下のいずれかを想定する。

| 言語 | TUIライブラリ | 備考 |
|---|---|---|
| Go | Bubble Tea | 単一バイナリ化しやすい |
| Rust | ratatui | 高速・堅牢 |
| Python | Textual / prompt_toolkit | 実装が速い |
| Kotlin/JVM | Lanterna | ユーザーの技術スタックに合う |

### 17.2 推奨構成

本実装ではRust + ratatuiを採用する。

```text
pve-vm-launcher/
├── Cargo.toml
├── src/
│   ├── main.rs
│   ├── app.rs
│   ├── command.rs
│   ├── config.rs
│   ├── proxmox.rs
│   ├── viewer.rs
│   └── ui.rs
└── README.md
```

Go実装の場合:

```text
pve-vm-launcher/
├── cmd/
│   └── pve-vm-launcher/
│       └── main.go
├── internal/
│   ├── proxmox/
│   │   ├── qm.go
│   │   ├── pvesh.go
│   │   └── parser.go
│   ├── viewer/
│   │   ├── vnc.go
│   │   └── spice.go
│   ├── tui/
│   │   ├── app.go
│   │   ├── list.go
│   │   └── menu.go
│   ├── config/
│   │   └── config.go
│   └── log/
│       └── logger.go
└── README.md
```

## 18. 主要フロー

### 18.1 起動フロー

```text
アプリ起動
  ↓
依存コマンド確認
  ↓
Proxmoxノード名取得
  ↓
VM一覧取得
  ↓
TUI表示
```

### 18.2 VM起動フロー

```text
VM選択
  ↓
Start選択
  ↓
qm start {vmid}
  ↓
状態ポーリング
  ↓
一覧更新
```

### 18.3 VNC接続フロー

```text
VM選択
  ↓
Attach via VNC
  ↓
VM running確認
  ↓
pvesh create /nodes/{node}/qemu/{vmid}/vncproxy
  ↓
port/ticket取得
  ↓
Remminaプロファイル生成
  ↓
remmina -c profile起動
  ↓
一時ファイル削除予約
```

### 18.4 SPICE接続フロー

```text
VM選択
  ↓
Attach via SPICE
  ↓
VM running確認
  ↓
pvesh create /nodes/{node}/qemu/{vmid}/spiceproxy --output-format json
  ↓
JSONを[virt-viewer]形式の.vvへ変換
  ↓
remote-viewer file.vv 起動
  ↓
.vv削除予約
```

## 19. MVP要件

初期実装では以下を満たせばよい。

### 19.1 必須機能

- VM一覧表示
- VMID / name / status表示
- 一覧更新
- VM起動
- VMシャットダウン
- VM強制停止
- VNC接続試行
- SPICE接続
- エラー表示
- 一時ファイル削除

### 19.2 MVPで省略可能な機能

- 詳細なCPU/メモリ表示
- クラスタ複数ノード対応
- スナップショット管理
- 設定ファイル
- ログビューアー
- 自動接続方式判定
- Remmina以外のVNCビューアー対応

## 20. 将来拡張

### 20.1 複数ノード対応

クラスタ環境ではVMが複数ノードに存在するため、以下に対応する。

```bash
pvesh get /cluster/resources --type vm --output-format json
```

表示例:

```text
NODE       VMID  NAME        STATUS
pve01      100   win11-dev   running
pve02      101   ubuntu-lab  stopped
```

### 20.2 検索・フィルタ

- VM名検索
- runningのみ表示
- stoppedのみ表示
- タグによるフィルタ

### 20.3 プロトコル優先設定

VMごとに接続方式を記憶する。

```toml
[vm.100]
preferred_protocol = "spice"

[vm.101]
preferred_protocol = "vnc"
```

### 20.4 Viewer差し替え

VNCビューアーを差し替え可能にする。

候補:

- Remmina
- TigerVNC Viewer
- virt-viewer
- noVNC + websockify

### 20.5 systemd service連携

ホストログイン時に自動起動するため、systemd user serviceを提供する。

```text
~/.config/systemd/user/pve-vm-launcher.service
```

## 21. 非機能要件

### 21.1 応答性

- VM一覧取得は通常1秒以内
- TUI操作は100ms以内に反応
- ビューアー起動は5秒以内に開始

### 21.2 安定性

- 外部コマンド失敗時にTUIをクラッシュさせない
- VM操作中も一覧更新できる
- 一時ファイルは起動時にクリーンアップする

### 21.3 保守性

- Proxmox操作層とTUI層を分離する
- VNC接続処理とSPICE接続処理を分離する
- コマンド実行処理を共通化する
- パース処理はテスト可能にする

## 22. 受け入れ条件

### 22.1 VM一覧

- `qm list`と同等のVM一覧が表示される
- VMID、VM名、状態が表示される
- `r`で更新できる

### 22.2 VM操作

- stopped VMを起動できる
- running VMをシャットダウンできる
- running VMを強制停止できる
- 破壊的操作には確認が出る

### 22.3 SPICE接続

- running VMに対して`.vv`ファイルを生成できる
- `remote-viewer`が起動する
- 起動後に一時`.vv`ファイルが削除される

### 22.4 VNC接続

- running VMに対してVNC proxyを起動できる
- Remmina用プロファイルを生成できる
- Remminaが起動する
- 接続失敗時に原因候補を表示する

### 22.5 セキュリティ

- APIトークンを保存しない
- 認証情報を含む一時ファイルを永続化しない
- 一時ファイルのパーミッションが`0600`相当である

## 23. 実装上の注意

### 23.1 VNCは最も相性問題が出やすい

ProxmoxのVNCは、単純な`localhost:5900`型の常時VNCではない。

Remminaでの接続は環境により失敗する可能性があるため、VNCは初期実装ではexperimentalとして扱う。

### 23.2 SPICEを優先する

SPICE対応VMではSPICEを優先する。

理由:

- `.vv`ファイルを`remote-viewer`へ渡すだけでよい
- Proxmox標準のSPICE接続方式と近い
- VNCより実装が単純
- 画面転送性能や入力周りが良い場合が多い

### 23.3 root前提を明示する

APIトークンを使わない代わりに、ホスト上でroot権限を使う。

これは仕様上のトレードオフである。

### 23.4 ホストGUI環境の安全性

ProxmoxホストへGUI、RDP、VNCサーバー、ブラウザ、ビューアーを入れると攻撃面は増える。

本アプリケーションは以下の環境を主対象とする。

- ホームラボ
- 検証環境
- 教材環境
- ローカル管理端末兼用のProxmoxホスト

本番クラスタや外部公開環境では、別管理端末からProxmox Web UIを利用する方式を推奨する。
