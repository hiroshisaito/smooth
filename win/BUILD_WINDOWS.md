# smooth-mod-v1.5.0 / Phase 2-D Windows ビルド手順

**対象マシン**: Windows 10/11 + Visual Studio 2017 以降 + Adobe After Effects 2025 SDK。
(Mac から Phase 2-D のソース準備のみ済み。本ビルド・AE 動作確認は Windows 機で実施してください。)

## 前提

- ブランチ: `feature/smooth-mod-phase2`(Phase 1 のコミットをすべて含む)
- タグ `v1.5.0` は Phase 1 Mac 版の釘として打ってある
- 新規ファイル: [smooth_core.h](../smooth_core.h), [bench.h](../bench.h)
- [Pipl.r](../Pipl.r) に `CodeMacARM64` 追加済み(Win ビルドでは無視される想定)

## 1. AE SDK を展開

Windows 機側で AE2025 SDK を配置:
```
C:\AE_SDK\ae25.6_61.64bit.AfterEffectsSDK\
    Examples\
        Headers\
        Headers\SP\
        Util\
        Resources\
```

## 2. Visual Studio でソリューションを開く

- `win\win.sln` を VS2017 / VS2019 / VS2022 で開く
- v141(VS2017)のツールセットが要求される場合は、VS Installer で **「C++ v141 ビルドツール」** をインストール。または `PlatformToolset` を **v142**(VS2019) / **v143**(VS2022)に一括変更。
  - `win\win.vcxproj` 内 `<PlatformToolset>v141</PlatformToolset>` × 4 箇所。

## 3. SDK パスを通す

`win.vcxproj` は `$(SDKPath)` 環境変数経由で AE SDK を参照しています。 以下のどちらかで設定:

**ユーザー環境変数**:
- `SDKPath` = `C:\AE_SDK\ae25.6_61.64bit.AfterEffectsSDK\`

**または VS プロパティページで追加**:
- `C/C++` → `全般` → `追加のインクルードディレクトリ`:
  - `$(SDKPath)Examples\Headers;$(SDKPath)Examples\Headers\SP;$(SDKPath)Examples\Util;$(SDKPath)Examples\Resources;%(AdditionalIncludeDirectories)`

## 4. ビルド

- 構成: **Release**
- プラットフォーム: **x64**(Windows AE は実質 x64 のみ)
- ビルド → Release/x64/ に `win.aex` が生成されます

Phase 1 で入った `std::thread` ベース並列化は VS2017+ の MSVC で問題なく動作するはずです。コンパイルエラーが出たらよくある原因:
- AE SDK ヘッダの `PF_Pixel` 等が見つからない → インクルードパス未通
- `_mkdir` 未定義 → `bench.h` の `<direct.h>` include 確認(SMOOTH_BENCH 定義時のみ必要)
- ランタイムライブラリ設定 → `/MD`(DLL ランタイム)推奨

## 5. AE プラグインとして配置

ビルドした `win.aex` を以下へコピー(配布時はファイル名を `smooth.aex` にリネーム推奨):

```
C:\Program Files\Adobe\Adobe After Effects 2025\Support Files\Plug-ins\Effects\smooth.aex
```

## 6. AE 動作確認

AE 2025 を起動 → 新規合成 → エフェクト → `LoiLo > smooth` を適用。
Mac 側と同じ UI・パラメータ(range / line weight / white option)で動作すること。

## 7. 期待パフォーマンス

Windows 側でも Phase 1 の並列化が効くため、Mac と同オーダの speedup が期待できます:
- HD 1920×1080 16bpc: **5〜10 ms 目標**(CPU コア数依存)
- 4K 3840×2160 16bpc: **30 ms 前後**

## 8. ベンチ動作確認(optional)

Windows でベンチ計測したい場合は `SMOOTH_BENCH=1` を `C/C++` → `プリプロセッサの定義` に追加して Release ビルド。

dump 出力先は `C:\Temp\smooth_bench\`(`bench.h` に Windows ガードで設定済)。

## 9. 回帰テスト(optional, Windows)

`tests/regression_test.cpp` は POSIX ヘッダに依存しない形になっていますが、`run_regression.sh` は bash スクリプト。Windows では WSL または Git Bash を使うか、Python に移植してください(Phase 2 以降の課題)。

## リリース成果物

配布用 zip はビルド後に Windows 側で作成:

```cmd
powershell Compress-Archive -Path Release\x64\smooth.aex -DestinationPath smooth.Win.1.5.0.AE2025.x64.zip
```

## 既知の未対応事項

- Windows ARM64 (AE for Windows on ARM) サポートは未対応(AE 2025 の Windows ARM64 対応状況が不明瞭なため見送り)
- `PlatformToolset=v141` は将来 v142/v143 に更新推奨
