# smooth 1.5.0 Windows ビルド手順(Phase 2-C + 2-D 対応)

**対象マシン**: Windows 10/11 + Visual Studio 2022 以降 + Adobe After Effects 2025 SDK + Rust stable。

Mac 側は既に Phase 2-C (Rust コア化) 済み。Windows 側も同じコードを走らせるため、**Rust staticlib のビルドが必須**です。

## 前提ツール

| ツール | 推奨バージョン | 備考 |
|---|---|---|
| Visual Studio | 2022 (v143) | v141 は未インストールなら v143 に統一済 |
| Windows SDK | 10.0.22621+ | vcxproj は `10.0`(自動選択)指定 |
| Rust | 1.70+ stable | `rustup` で入れる |
| AE SDK | 25.6.61 (AE 2025) | `references/AfterEffectsSDK_25.6_61_win/` に配置済 |

## 1. Rust toolchain 準備(初回のみ)

```cmd
rustup target add x86_64-pc-windows-msvc
```

静的 CRT 設定は [rust/smooth_core/.cargo/config.toml](../rust/smooth_core/.cargo/config.toml) でプロジェクト内に固定済み(`target-feature=+crt-static`、MSVC target 限定なので Mac ビルドには無影響)。

## 2. AE SDK パスを通す

`win.vcxproj` は `$(SDKPath)` 環境変数経由で AE SDK を参照します。

**ユーザー環境変数(推奨)**:
- `SDKPath` = `D:\GitHub\smooth\references\AfterEffectsSDK_25.6_61_win\ae25.6_61.64bit.AfterEffectsSDK\Examples\`(末尾 `\` 必須)

外部配置する場合は `C:\AE_SDK\ae25.6_61.64bit.AfterEffectsSDK\Examples\` など、`Examples\` までのパスで OK。

## 3. Visual Studio でビルド

- `win\win.sln` を VS2022 で開く
- 構成: **Release** / プラットフォーム: **x64**
- ビルド開始 → PreBuildEvent が [rust/smooth_core/build-windows.bat](../rust/smooth_core/build-windows.bat) を呼んで Rust staticlib (`smooth_core.lib`) を生成 → C++ が静的リンク
- 出力: `win\Release\x64\smooth.aex`(約 390 KB、Rust rayon + std 含む)

### コマンドラインから
```cmd
set SDKPath=D:\GitHub\smooth\references\AfterEffectsSDK_25.6_61_win\ae25.6_61.64bit.AfterEffectsSDK\Examples\
call "<VS2022>\VC\Auxiliary\Build\vcvars64.bat"
msbuild win\win.sln /p:Configuration=Release /p:Platform=x64 /m
```

## 4. AE プラグインとして配置

```cmd
copy win\Release\x64\smooth.aex "D:\Program Files\Adobe After Effects 2025\Support Files\Plug-ins\Effects\smooth.aex"
```
(インストール先が `C:\Program Files\...` の場合は適宜読み替え)

## 5. AE 動作確認

AE 2025 を起動 → 新規合成 → エフェクト → `LoiLo > smooth`。range / line weight / white option が Mac 版と同挙動であることを確認。

## 6. 配布 zip 作成

```cmd
powershell Compress-Archive -Path win\Release\x64\smooth.aex -DestinationPath win\release\smooth.Win.1.5.0.AE2025.x64.zip
```

## 既知事項 / トラブルシュート

| 症状 | 対処 |
|---|---|
| `Cannot open include file: 'smooth_core_ffi.h'` | Rust staticlib のビルドが走っていない。`rust\smooth_core\target\x86_64-pc-windows-msvc\release\smooth_core.lib` の存在を確認。`build-windows.bat` を手で叩く |
| `LNK2038: mismatch detected for 'RuntimeLibrary'` | `/MT` (MultiThreaded static) と `/MD` が混在。C++ Release は `/MT`、Rust も `+crt-static` で `libcmt` 静的。両方 `/MT` 系に揃っていること |
| `winnt.h static_assert failed: default packing option` | `<StructMemberAlignment>` が `4Bytes` になっていないか確認(`Default` でないと Win11 SDK で失敗) |
| `'strlcpy': identifier not found` | `Effect.cpp` で `#include "AEConfig.h"` が抜けている。AE_Effect.h より前に入れる |
| `error C2589: '(': illegal token on right side of '::'` (std::min/max) | `NOMINMAX` が Preprocessor Definitions にあるか確認 |
| `vcvars64.bat` で `\Common was unexpected at this time.` | 環境変数 `PATH` に空白を含むパス(MS Office の `Common` 等)があると vcvars 内部の `if` が誤動作。最小 env で cmd を起動 |

## 回帰テスト(optional)

`tests/regression_test.cpp` は POSIX 非依存。`run_regression.sh` は bash なので WSL / Git Bash 必要。Windows 実行は現状未サポート。

## 既知の未対応事項

- Windows ARM64 は未対応(需要次第)
- Debug x64 は `RuntimeLibrary` を `MultiThreaded` に固定(Rust 静的 CRT に合わせるため、デバッグ CRT は使えない)
