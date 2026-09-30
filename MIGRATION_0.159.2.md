# rust-v0.159.2 への移行記録

更新日: 2026-10-01

## 採用した基点

- 公式リポジトリ: [openai/codex](https://github.com/openai/codex)
- 公式リリース: [`rust-v0.159.2`](https://github.com/openai/codex/releases/tag/rust-v0.159.2)
- 公式コミット（注釈付きタグの dereference 先）: `ff6aec96948b70d94983af2641a6b67c94faeff5`
- 作業場所: `/Users/nonaka/src/convenient-codex-0.159.2`
- ブランチ: `port/rust-v0.159.2`
- 製品改訂: `0.159.2+convenient.1`

前の製品版 `0.156.1+convenient.1`（コミット `425a18e4e6d6bd63d23d50501886beb4a9e018a3`）から、公式の
`rust-v0.159.2` をマージして移植した。旧版の全体は `archive/custom-0.153.4` に保存している。
0.156.1 との差分は公式側だけで約 2,200 ファイルに及ぶため、競合を解消しながら独自機能を再確認した。

## 維持した独自仕様

- `env_switch` / `env_status` / `env_list` と、スレッド単位の実行環境選択。
- `monitor` / `monitor_realtime`、要約監視・即時監視のライフサイクル、TUI 表示。
- Goal の `goalWait`、待機状態の保存、Monitor があるだけでは Goal を停止しない継続条件。
- app-server の Guardian 回路遮断、モデルカタログ要件、製品 manifest と専用インストーラー。

公式側で HTTP エラー型が統合されたため、旧版にあった重複したエラー変換を 1 箇所整理した。これは実行時の
エラー種別を変えず、0.159.2 の共有型へ合わせるための互換修正である。

## 検証

専用ビルド領域 `/Volumes/CodexBuild20260912` を使い、`codex-voice-host`（macOS の GStreamer 開発 SDK が
別途必要なため）を除く workspace のコンパイル検査を実行し、通過した。独自機能の対象テストも次の結果になった。

- `cargo check --workspace --tests --exclude codex-voice-host`: 成功（既存コード由来の警告のみ）。
- `just test -p codex-core --lib monitor`: 9 件成功。
- `just test -p codex-core --lib env_switch`: 51 件成功（既存テストの leaky 1 件を含む）。
- `just test -p codex-goal-extension`: 41 件成功。
- `cargo build -p codex-cli -p codex-code-mode-host`: dev profile で成功。

## 導入

検証を通過したバイナリは、既存の認証・会話データを保持したまま、専用インストーラーで
`~/.local/share/convenient-codex/releases/0.159.2+convenient.1` に配置し、`current` をこの版へ切り替えた。
導入時の source commit はこの移行コミット、未コミット変更なしとして記録されている。
`codex --build-info` は upstream `rust-v0.159.2`、製品版 `0.159.2+convenient.1`、全機能一覧を表示し、
`codex --version` は `codex-cli 0.159.2` になった。`codex-code-mode-host --help` と既存の ChatGPT ログイン状態
（`Logged in using ChatGPT`）も確認した。`codex-pool` の既存ランチャーは保持し、current の新しいバイナリを使う状態にした。

旧版へ戻す場合は、版別ディレクトリを削除せず `current` symlink を旧版へ戻す。
