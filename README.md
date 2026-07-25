# VoxShift

VRChatのマイクミュート状態とDiscordのセルフミュート状態を連動させる、Windows向けのタスクトレイ常駐アプリです。
VRChat側でマイクをオン/オフにすると自動的にDiscordのセルフミュートが切り替わり（またはその逆）、
どちらか片方だけを操作する手間をなくします。

- [プライバシーポリシー](https://glaz-egy.github.io/VoxShift/privacy.html)
- [利用規約](https://glaz-egy.github.io/VoxShift/terms.html)

## 主な機能

- VRChatのOSC（Open Sound Control）を利用したマイクミュート状態の検知・切り替え
- Discordの公式デスクトップアプリとのRPC通信によるセルフミュート状態の取得・変更
- 連動モードの切り替え（双方向 / VRChat優先）
- 一時停止・手動再同期
- タスクトレイ常駐（ウィンドウを閉じても終了しない）
- 日本語 / English 表示切り替え（あとから言語パックを追加できる仕組み）
- Discordの認証情報はWindowsの資格情報マネージャーに保存され、外部サーバーには一切送信されません

## 動作環境

- Windows 10 / 11
- [Discord](https://discord.com/) デスクトップアプリ（ブラウザ版では動作しません）
- OSCを有効にした [VRChat](https://hello.vrchat.com/)

## 重要な注意事項（Discordの音声設定スコープについて）

DiscordのRPC APIで音声のミュート状態を取得・変更するには `rpc.voice.read` / `rpc.voice.write`
スコープが必要ですが、これらはDiscordが個別に承認したアプリケーション（テスター登録済みのユーザー、最大50人）
でしか利用できません。本リポジトリのビルド済みDiscordクライアントIDを使ってご自身でビルド・実行する場合、
開発者（glaz-egy）に連絡してテスターとして登録してもらうか、
[Discord Developer Portal](https://discord.com/developers/applications) で独自のアプリケーションを作成し、
`.cargo/config.toml` の `VOXSHIFT_DISCORD_CLIENT_ID` を書き換えたうえで、
そのアプリの「Rich Presence > Whitelist」からご自身のDiscordアカウントをテスターとして追加してください。

## ビルド方法

Rust（[rustup](https://rustup.rs/)）がインストールされている環境で以下を実行してください。
利用するツールチェーンのバージョンは `rust-toolchain.toml` で固定されています。

```
git clone https://github.com/glaz-egy/VoxShift.git
cd VoxShift
cargo build --workspace --release
```

ビルドされた実行ファイルは `target\release\voxshift.exe` に生成されます。

## 使い方

1. VRChatの `Settings > OSC` でOSCを有効化し、`Settings > Audio` の「Toggle Voice」をONにしてください
   （OFFのままだとマイクのオン/オフが正しく連動しません）
2. Discordデスクトップアプリを起動しておいてください
3. VoxShiftを起動すると、タスクトレイに常駐します
4. 設定画面を開き、「Discordで認証」ボタンを押してDiscordの認証を行ってください
   （初回起動時に自動で認証は行われません。ボタンを押した時のみ認証ダイアログが表示されます）
5. 認証後、ダッシュボード画面で連動モードや一時停止などを操作できます

## ライセンス

本リポジトリは [MITライセンス](LICENSE) のもとで公開されています。

本アプリはDiscord Inc.およびVRChat Inc.と提携・承認・後援関係にありません。
