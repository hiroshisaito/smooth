# smooth — メンテナンス fork(smooth-mod)

本リポジトリは [loilo-inc/smooth](https://github.com/loilo-inc/smooth)(オリジナル upstream)の **メンテナンス fork** で、現行 Adobe After Effects (2025 以降) への対応、Multi-Frame Rendering 対応、Windows ビルド復活を主眼としています。

オリジナルの配布履歴・背景・日本語の README はこちら:
**https://github.com/loilo-inc/smooth** — [オリジナル README](https://github.com/loilo-inc/smooth/blob/master/README.md)。

---

## smooth とは

**smooth** はドット絵風アニメーションのスムージング(回転・拡大縮小で生じるジャギーを滑らかに、ハードエッジ感は保ちつつ)を行う After Effects プラグインです。LoiLo 株式会社が有償販売していたものを Apache 2.0 でオープンソース化したのが upstream です。

インストール後、AE のエフェクトメニュー `LoiLo > smooth` として利用できます。

## この fork で追加した変更点(upstream 1.4.0 比)

| 項目 | upstream (1.4.0) | 本 fork (v1.6.0) |
| --- | --- | --- |
| After Effects | CC2017 向け | **AE 2025**(SDK 25.6.61)対応、旧 AE は未検証 |
| Apple Silicon | 非対応(Intel のみ) | **Universal binary**(arm64 + x86_64)、PiPL エントリ修正済 |
| コア実装 | `Effect.cpp` 内の C++ テンプレート | **Rust コア**(`rust/smooth_core/`)、AE SDK 非依存、thread-safe by construction |
| フレーム内並列化 | シングルスレッド | **rayon による行ブロック並列**(Phase 2-C) — HD 16bpc で 20 ms → 7 ms(2.9×) |
| フレーム間並列化 | なし | **Multi-Frame Rendering**(`SUPPORTS_THREADED_RENDERING`、Phase 2-B) — AE が複数フレームを同時レンダー |
| ビット深度 | 8/16bpc | **8/16/32bpc 対応**(Phase 2-A.2) — 32bpc Comp / HDR / float color workflow |
| Windows | 旧 SDK 向けでビルド不能 | AE 2025 SDK + VS2022 + `+crt-static` Rust で復活 |
| build-id 表示 | なし | Effect Controls に `Build: 0.1.0+<git sha>` 常時表示 + About ダイアログでフォルスサクセス検知 |

## ダウンロード

バイナリは GitHub の各タグのリリースページから配布しています:

**https://github.com/hiroshisaito/smooth/releases**

最新安定版: **v1.6.0**(32bpc 対応 + Multi-Frame Rendering、CPU only)。

配布 zip の一覧(SHA256 ゴールド値は [`RELEASE_NOTES-v1.6.0.md`](RELEASE_NOTES-v1.6.0.md) 参照):

| プラットフォーム | アーカイブ |
| --- | --- |
| macOS (universal: Apple Silicon + Intel) | `smooth.Mac.1.6.0.AE2025.universal.zip` |
| macOS (arm64 のみ) | `smooth.Mac.1.6.0.AE2025.arm64.zip` |
| macOS (x86_64 のみ) | `smooth.Mac.1.6.0.AE2025.x86_64.zip` |
| Windows x64 | `smooth.Win.1.6.0.AE2025.x64.zip` |

新規/再作成する配布 zip にはプラグイン本体に加えて `LICENSE` と `THIRD_PARTY_LICENSES.md` を同梱します。`references/` 配下の Adobe After Effects SDK、展開ツール、その他 vendor SDK/toolchain 類は配布物に含めません。

## インストール

### macOS

```sh
unzip smooth.Mac.1.6.0.AE2025.universal.zip
sudo cp -R smooth.plugin "/Applications/Adobe After Effects 2025/Plug-ins/Effects/"
```

### Windows

zip を展開して `smooth.aex` を以下にコピーしてください:

```
C:\Program Files\Adobe\Adobe After Effects 2025\Support Files\Plug-ins\Effects\
```

### インストール確認(3 段偽成功検証)

AE を再起動して任意のレイヤーに `LoiLo > smooth` を適用、Effect Controls を開いて:

1. **Build** キャプションに `0.1.0+<git sha>` が表示される → 古いビルドが残っていないことを確認(SHA は `RELEASE_NOTES-v1.6.0.md` の build-id を参照)
2. **Build** キャプションをクリックすると About ダイアログが開き、`smooth, v1.6.0` + `rust_core 0.1.0+<sha> ffi=0x00020003` が表示される
3. AE 起動時・プロジェクト読込時に verification-failure ダイアログが出ない → MFR flag が正しく同期されていることを確認

## ソースからのビルド

### 共通前提

- [Adobe After Effects SDK 25.6.61](https://developer.adobe.com/console/servicesandapis)(`references/AfterEffectsSDK_25.6_61_<mac|win>/` 配下に配置)
- [Rust stable 1.95 以上](https://rustup.rs/)(`rust/smooth_core/rust-toolchain.toml` で固定)

`references/` はローカルビルド用の配置場所です。SDK/toolchain の再配布条件は各 vendor の利用規約に従い、通常の smooth ソース配布・バイナリ配布には含めません。

### macOS

```sh
xcodebuild -project Mac/smooth.xcodeproj \
           -configuration Release \
           -arch x86_64 -arch arm64 ONLY_ACTIVE_ARCH=NO \
           clean build
```

出力: `Mac/build/Release/smooth.plugin`(universal Mach-O)。

Xcode ビルド中に `rust/smooth_core/build-universal.sh` が走り、`libsmooth_core.a` を lipo した universal 静的ライブラリとして生成します。

検証環境: Xcode 26.3 / macOS SDK 26.2 / Apple Silicon + Intel。

### Windows

Visual Studio 2022 で `win/smooth.sln` を開くか、コマンドラインで:

```bat
msbuild win\smooth.sln /p:Configuration=Release /p:Platform=x64 /t:Rebuild
```

出力: `win\Release\x64\smooth.aex`。

MSBuild から `rust/smooth_core/build-windows.bat` が呼ばれ、`+crt-static` で MSVC 互換の `smooth_core.lib` を生成します。

検証環境: Windows 10 Pro / VS2022 v143 (MSVC 19.44.35225) / Windows SDK 10.0.26100.0。

## リポジトリ構成

```
.
├── Effect.cpp / Effect.h          # AE プラグイン本体(GlobalSetup、Render dispatch)
├── Pipl.r                         # AE PiPL リソース(Mac/Win 共通ソース)
├── rust/smooth_core/              # Rust コア: preprocess + smoothing + FFI
├── Mac/                           # Xcode project + リリース配布物(gitignored)
├── win/                           # VS2022 project + リリース配布物(gitignored)
├── THIRD_PARTY_LICENSES.md        # Rust 依存関係と SDK/toolchain の third-party notices
├── RELEASE_NOTES-v1.x.x.md        # 各リリースの詳細 + SHA256 ゴールド
├── workbench_history.md           # Phase/Step 単位の開発ログ(日本語)
└── docs/                          # 運用ドキュメント(build-id 検証手順等)
```

## リリース履歴

- **v1.6.0** — 32bpc(PF_PixelFloat / float color)対応。SmartRender 経路化 + 32bpc Comp / HDR / float color workflow に対応(CPU only)。[リリースノート](RELEASE_NOTES-v1.6.0.md)
- **v1.5.1** (2026-04-22) — Multi-Frame Rendering + build-id UI 対応。[リリースノート](RELEASE_NOTES-v1.5.1.md)
- **v1.5.0** (2026-04-21, Phase 2-D 時点) — Rust コア移行 + Windows AE 2025 対応の統合
- v1.5.0 (初出) — AE 2025 対応 + rayon 並列化 + Apple Silicon 対応
- (この fork では未リリース)upstream smooth 1.4.0 — LoiLo 株式会社による AE CC2017 向け初版

フェーズ単位の詳細開発ログ: [`workbench_history.md`](workbench_history.md)

## 開発ノート

- 各 Phase/Step は commit 前に [`workbench_history.md`](workbench_history.md) へ追記するルール(同ファイル冒頭に明記)
- [`docs/WINDOWS_BUILD_ID_INTEGRATION.md`](docs/WINDOWS_BUILD_ID_INTEGRATION.md) に build-id UI の検証手順と LTO インライン化対策を記載
- 配布バイナリの SHA256 は非決定性(MSVC linker timestamp / Mac codesign timestamp のため)。ゴールド SHA と一致しない再ビルド binary の等価性は、Build キャプション + `EntryPointFunc` unmangled export + 3 段偽成功検証 で確認可能(`workbench_history.md` の「等価性検証手順」セクション参照)

## ライセンス

Apache License 2.0([upstream](https://github.com/loilo-inc/smooth) から継承)。[`LICENSE`](LICENSE) 参照。

Rust 依存関係は MIT / Apache-2.0 / MIT OR Apache-2.0 系の permissive license が中心で、build-time の `unicode-ident` は Unicode License v3 notice も必要です。詳細な third-party notices は [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md) に記載しています。

## クレジット

- **upstream**: [LoiLo 株式会社](https://loilo.tv/) — smooth プラグイン原作者(Koji Kobayashi 氏ほか)、Apache 2.0 で https://github.com/loilo-inc/smooth にて公開
- **本 fork**: [Hiroshi Saito](https://github.com/hiroshisaito) によるメンテナンス作業、Claude (Anthropic) とのペアプログラミング(各 commit の Co-Authored-By 参照)
