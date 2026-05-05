
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
#include "AE_EffectCBSuites.h"   // Phase 2-A.2: PF_WorldSuite2 / kPFWorldSuite for PF_GetPixelFormat (32bpc detection)
#include "AE_EffectGPUSuites.h"  // Phase 2-A.3 C-2.5a: PF_GPUDeviceSuite1 (GetDeviceInfo / GetGPUWorldData)
#include "SPBasic.h"             // SPBasicSuite (in_data->pica_basicP) AcquireSuite/ReleaseSuite
#include "AE_Macros.h"

#include "Param_Utils.h"
#include "version.h"
#include "util.h"


#include "define.h"

#include "upMode.h"
#include "downMode.h"
#include "8link.h"
#include "Lack.h"

#include "Effect.h"
#include "bench.h"
#include "smooth_core.h"
#include "smooth_core_ffi.h"

//---------------------------------------------------------------------------//
// 定義
enum
{
    PARAM_INPUT = 0,
    PARAM_WHITE_OPTION,
    PARAM_RANGE,
    PARAM_LINE_WEIGHT,
    PARAM_BUILD_INFO,         // 読み取り専用の Build 表示(偽成功判別用)
    PARAM_GPU_ACCELERATION,   // Phase 2-A.3 Sub-stage C-2: GPU on/off checkbox
    PARAM_NUM,
};

// Phase 2-A.1: SmartRender が PreRender → Render の 2 段階になるため、
// 非 layer params の値を PreRender 時点で snapshot してから Render に渡す。
// pre_render_data は AE が delete_pre_render_data_func 経由で解放する。
// Phase 2-A.3 C-2: gpu_acceleration を追加(checkbox 状態 snapshot)。
struct SmartRenderInfo
{
    double range;              // PARAM_RANGE.fs_d.value
    double line_weight;        // PARAM_LINE_WEIGHT.fs_d.value
    bool   white_option;       // PARAM_WHITE_OPTION.bd.value
    bool   gpu_acceleration;   // PARAM_GPU_ACCELERATION.bd.value
};

// Phase 2-A.3 Sub-stage C-2: per-instance sequence_data layout.
// Stored in a PF_Handle allocated by SEQUENCE_SETUP, regenerated at every
// SETUP / RESETUP per RFC §6.5 (UUID never carries across save/load).
// The DashMap GPU_FALLEN entry is keyed by this UUID.
struct SequenceData
{
    uint64_t uuid_lo;
    uint64_t uuid_hi;
};

// Phase 2-A.3 dispatch gate.
//
// Flipped from 0 to 1 in Sub-stage C-2.5a once the Metal command-buffer
// round-trip (GPU device suite -> MTLBuffer -> identity passthrough kernel
// -> GPU output world) was wired end-to-end. With this set:
//   * SmartPreRender's 5-condition AND raises GPU_RENDER_POSSIBLE on 32bpc
//     comps that satisfy the other conditions, so AE dispatches
//     SMART_RENDER_GPU.
//   * SmartRenderGpu acquires kPFGPUDeviceSuite, obtains MTLBuffer pointers
//     from the input/output GPU effect worlds, and calls
//     smooth_core_metal_dispatch_passthrough. The kernel itself is still
//     identity (Sub-stage C-1's smooth.metal `smooth_passthrough`); the
//     real 2-pass smooth lands in C-2.5b.
//
// While this is 0 the GPU plumbing is fully wired but dormant — every
// selector still works, fault injection still fires, but AE never issues
// SMART_RENDER_GPU because PreRender suppresses the flag.
#ifndef SMOOTH_GPU_DISPATCH_READY
#define SMOOTH_GPU_DISPATCH_READY 1
#endif

//---------------------------------------------------------------------------//
// プロトタイプ
static PF_Err About (   PF_InData       *in_data,
                        PF_OutData      *out_data,
                        PF_ParamDef     *params[],
                        PF_LayerDef     *output );

static PF_Err GlobalSetup ( PF_InData       *in_data,
                            PF_OutData      *out_data,
                            PF_ParamDef     *params[],
                            PF_LayerDef     *output );

static PF_Err GlobalSetdown( PF_InData      *in_data,
                            PF_OutData      *out_data,
                            PF_ParamDef     *params[],
                            PF_LayerDef     *output );

static PF_Err ParamsSetup ( PF_InData       *in_data,
                            PF_OutData      *out_data,
                            PF_ParamDef     *params[],
                            PF_LayerDef     *output);

static PF_Err Render (  PF_InData       *in_data,
                        PF_OutData      *out_data,
                        PF_ParamDef     *params[],
                        PF_LayerDef     *output );

static PF_Err PopDialog (PF_InData		*in_data,
						 PF_OutData		*out_data,
						 PF_ParamDef		*params[],
						 PF_LayerDef		*output );

// Phase 2-A.1: SmartRender 三本化(legacy PF_Cmd_RENDER は維持、追加で
// PF_Cmd_SMART_PRE_RENDER / PF_Cmd_SMART_RENDER を実装)。GPU 経路
// (PF_Cmd_SMART_RENDER_GPU)は Phase 2-A.3 Sub-stage C-2 で追加(stub、
// 実 Metal dispatch は C-2.5)。
static PF_Err SmartPreRender(PF_InData         *in_data,
                             PF_OutData        *out_data,
                             PF_PreRenderExtra *extraP);

static PF_Err SmartRender(PF_InData            *in_data,
                          PF_OutData           *out_data,
                          PF_SmartRenderExtra  *extraP);

static PF_Err SmartRenderGpu(PF_InData            *in_data,
                             PF_OutData           *out_data,
                             PF_SmartRenderExtra  *extraP);

// Phase 2-A.3 Sub-stage C-2: sequence_data 8 selector. UUID lifecycle is
// owned by these handlers; SMART_RENDER_GPU / SmartPreRender just read the
// UUID via SequenceDataReader below.
static PF_Err SequenceSetup     (PF_InData *in_data, PF_OutData *out_data);
static PF_Err SequenceResetup   (PF_InData *in_data, PF_OutData *out_data);
static PF_Err SequenceFlatten   (PF_InData *in_data, PF_OutData *out_data);
static PF_Err SequenceSetdown   (PF_InData *in_data, PF_OutData *out_data);
static PF_Err GetFlattenedSequenceData(PF_InData *in_data, PF_OutData *out_data);
static PF_Err GpuDeviceSetup    (PF_InData *in_data, PF_OutData *out_data, void *extra);
static PF_Err GpuDeviceSetdown  (PF_InData *in_data, PF_OutData *out_data, void *extra);


//---------------------------------------------------------------------------//
// util funcs
//---------------------------------------------------------------------------//
static inline void getWhitePixel(PF_Pixel16 *white)	
{ 
	PF_Pixel16 color = { 0x8000, 0x8000, 0x8000, 0x8000 };
	*white = color;
}

static inline void getWhitePixel(PF_Pixel8 *white )
{ 
	PF_Pixel8	color = { 0xFF, 0xFF, 0xFF, 0xFF };
	*white = color;
}

static inline void getNullPixel(PF_Pixel16 *null_pixel )
{ 
	PF_Pixel16	color = { 0x0, 0x0, 0x0, 0x0 };
	*null_pixel = color;
}

static inline void getNullPixel(PF_Pixel8 *null_pixel )
{ 
	PF_Pixel8	color = { 0x0, 0x0, 0x0, 0x0 };
	*null_pixel = color;
}






#if 0
template<typename PackedPixelType > 
static inline void ColorKey( PackedPixelType *in_ptr, int row_bytes, int height )
{
    int         limit, t=0;
    PackedPixelType	key;
	getWhitePixel( &key );	// 0xff or 0xffff

    limit = (row_bytes / sizeof(PackedPixelType)) * height;

    for( t=0; t<limit; t++)
    {
        if( key == in_ptr[t] )
        {
			in_ptr[t] = 0;
        }
    }
}
#endif


template<typename PixelType > 
static inline void preProcess( PixelType *in_ptr, int row_bytes, int height, PF_Rect *rect, bool is_white_trans )
{
    PixelType key;
	PixelType null_pixel;
	getWhitePixel( &key );	// 0xff or 0x8000
	getNullPixel( &null_pixel );

	int width = (row_bytes / sizeof(PixelType));
	
	int		top=0, left=width, right=0, bottom=0;
	bool	top_found=false, left_found=false;


	if( is_white_trans )
	{
		// 白を抜く
		// Alphaチャンネルは無視して、色が白だったら抜く
		int t=0;
		for(int j=0; j<height; j++)
		{
			if( !top_found )
			{
				top = j;
			}

			for(int i=0; i<width; i++)
			{
				if( key.red == in_ptr[t].red &&
					key.green == in_ptr[t].green &&
					key.blue == in_ptr[t].blue )
				{
					// 抜き色
					in_ptr[t] = null_pixel;
				}
				else if( in_ptr[t].alpha == 0 )
				{
					// すでに抜かれている
				}
				else
				{
					top_found = true;
					left_found = true;

					if( left > i )
					{
						left = i;
					}

					if( right < i )
					{
						right = i;
					}

					if( bottom < j )
					{
						bottom = j;
					}
				}
				t++;
			}
		}
	}
	else
	{
		// 白を抜かずに、領域情報だけ取得
		int t=0;
		for(int j=0; j<height; j++)
		{
			if( !top_found )
			{
				top = j;
			}

			for(int i=0; i<width; i++)
			{
				if(!(key.red == in_ptr[t].red && key.green == in_ptr[t].green && key.blue == in_ptr[t].blue ) &&
					 in_ptr[t].alpha != 0 )
				{
					top_found = true;
					left_found = true;

					if( left > i )
					{
						left = i;
					}

					if( right < i )
					{
						right = i;
					}

					if( bottom < j )
					{
						bottom = j;
					}
				}
				t++;
			}
		}
	}

	if( top_found )
		rect->top = top;
	else
		rect->top = 0;

	if( left_found )
		rect->left = left;
	else
		rect->left = 0;

	rect->right = right+1;
	rect->bottom = bottom+1;


}


//---------------------------------------------------------------------------//
// 概要   : Effectメイン
// 関数名 : EffectPluginMain
// 引数   : 
// 返り値 : 
//---------------------------------------------------------------------------//
DllExport
PF_Err EntryPointFunc(    PF_Cmd          cmd,
                            PF_InData       *in_data,
                            PF_OutData      *out_data,
                            PF_ParamDef     *params[],
                            PF_LayerDef     *output,
                            void            *extra )
{
    PF_Err      err = PF_Err_NONE;

    try
    {
        switch (cmd)
        {
            case PF_Cmd_ABOUT:              // Aboutボタンを押した
                err = About(in_data,
                            out_data,
                            params,
                            output);
                break;


            case PF_Cmd_GLOBAL_SETUP:       // Global setup 読み込まれた時1度だけ呼ばれるはず
                err = GlobalSetup(  in_data,
                                    out_data,
                                    params,
                                    output);
                break;

            case PF_Cmd_GLOBAL_SETDOWN:     // Global setdown 終了時1度だけ呼ばれるはず
                err = GlobalSetdown(in_data,
                                    out_data,
                                    params,
                                    output);
                break;

            case PF_Cmd_PARAMS_SETUP:       // パラメータの設定
                err = ParamsSetup(  in_data,
                                    out_data,
                                    params,
                                    output);
                break;


            case PF_Cmd_RENDER:             // レンダリング(legacy、SmartRender 非対応 AE 向け)
                err = Render(   in_data,
                                out_data,
                                params,
                                output);
                break;

            case PF_Cmd_SMART_PRE_RENDER:   // Phase 2-A.1: SmartRender 入口
                err = SmartPreRender(in_data, out_data, (PF_PreRenderExtra*)extra);
                break;

            case PF_Cmd_SMART_RENDER:       // Phase 2-A.1: CPU SmartRender
                err = SmartRender(in_data, out_data, (PF_SmartRenderExtra*)extra);
                break;

            case PF_Cmd_SMART_RENDER_GPU:   // Phase 2-A.3 C-2: GPU SmartRender (stub)
                err = SmartRenderGpu(in_data, out_data, (PF_SmartRenderExtra*)extra);
                break;

            // Phase 2-A.3 Sub-stage C-2: sequence_data lifecycle.
            // RFC §6.5: UUID is regenerated at every SETUP / RESETUP and never
            // carried across save/load (the flattened bytes get overwritten).
            case PF_Cmd_SEQUENCE_SETUP:
                err = SequenceSetup(in_data, out_data);
                break;
            case PF_Cmd_SEQUENCE_RESETUP:
                err = SequenceResetup(in_data, out_data);
                break;
            case PF_Cmd_SEQUENCE_FLATTEN:
                err = SequenceFlatten(in_data, out_data);
                break;
            case PF_Cmd_SEQUENCE_SETDOWN:
                err = SequenceSetdown(in_data, out_data);
                break;
            case PF_Cmd_GET_FLATTENED_SEQUENCE_DATA:
                err = GetFlattenedSequenceData(in_data, out_data);
                break;

            // Phase 2-A.3 Sub-stage C-2: GPU device lifecycle.
            // GPU_DEVICE_SETUP is the per-device-level "do you accept this
            // device?" handshake — answer yes by OR-ing SUPPORTS_GPU_RENDER_F32
            // into out_data->out_flags2 (the third place this flag is required,
            // see GlobalSetup comment).
            case PF_Cmd_GPU_DEVICE_SETUP:
                err = GpuDeviceSetup(in_data, out_data, extra);
                break;
            case PF_Cmd_GPU_DEVICE_SETDOWN:
                err = GpuDeviceSetdown(in_data, out_data, extra);
                break;

			case PF_Cmd_DO_DIALOG:
				err = PopDialog(in_data,
								out_data,
								params,
								output);
				break;

            case PF_Cmd_USER_CHANGED_PARAM:
                // ユーザーがパラメータ(主にボタン)を操作した時。
                // Build ボタンをクリックしたら About ダイアログを出す。
                {
                    PF_UserChangedParamExtra *ucp = reinterpret_cast<PF_UserChangedParamExtra*>(extra);
                    if (ucp && ucp->param_index == PARAM_BUILD_INFO) {
                        err = About(in_data, out_data, params, output);
                    }
                }
                break;

        }
    }
    catch( APIErr   api_err )
    {   // APIがエラーを返した
        
        PrintAPIErr( &api_err ); // プリント

        err = PF_Err_INTERNAL_STRUCT_DAMAGED;
    }
    catch(...)
    {
        err = PF_Err_INTERNAL_STRUCT_DAMAGED;
    }


    return err;
}



//---------------------------------------------------------------------------//
// 概要   : Aboutボタンを押したときに呼ばれる関数
// 関数名 : About
// 引数   : 
// 返り値 : 
//---------------------------------------------------------------------------//
static PF_Err About (   PF_InData       *in_data,
                        PF_OutData      *out_data,
                        PF_ParamDef     *params[],
                        PF_LayerDef     *output )
{
#if SK_STAGE_DEVELOP
    const char *stage_str= "Debug";
#elif SK_STAGE_RELEASE
    const char *stage_str= "";
#endif

    char str[256];
    memset( str, 0, 256 );

    const uint32_t rust_ffi = smooth_core_version();
    const char *build_id    = smooth_core_build_id();

    sprintf(    out_data->return_msg,
                 "%s, v%d.%d.%d %s\nrust_core %s  ffi=0x%08x\n%s\n",
                NAME,
                MAJOR_VERSION,
                MINOR_VERSION,
                BUILD_VERSION,
                stage_str,
                build_id,
                rust_ffi,
                str );

    return PF_Err_NONE;
}



//---------------------------------------------------------------------------//
// 概要   : プラグインが読み込まれた時に呼ばれる関数
// 関数名 : GlobalSetup
// 引数   : 
// 返り値 : 
//---------------------------------------------------------------------------//
static PF_Err GlobalSetup ( PF_InData       *in_data,
                            PF_OutData      *out_data,
                            PF_ParamDef     *params[],
                            PF_LayerDef     *output )
{
    // versionをpiplとあわせないといけないの&&PiPlは直値のみ
    // 使いづらいから使わないので0固定。
    // build=1: Build 表示パラメータ(PARAM_BUILD_INFO)追加
    out_data->my_version    = PF_VERSION(2,0,0,1,0);

    // PF_OutFlag_I_WRITE_INPUT_BUFFER は **撤去**: AE 2025 の verifier が
    //   "{plugin with PF_OutFlag2_SUPPORTS_SMART_RENDER cannot set
    //   PF_OutFlag_I_WRITE_INPUT_BUFFER}" を出して GLOBAL_SETUP 直後に
    //   internal verification failure dialog → render 経路で SIGSEGV を起こす
    //   ことを Phase 2-A.1 Step 2 実機検証で確認(2026-05-03、AE 25.6.5x3)。
    //   SmartRender 採用時は input buffer を read-only として扱う必要があり、
    //   smooth_core::process() が要求する writable in_ptr は smoothing<>()
    //   内部で scratch buffer を allocate して提供する形に変更。
    out_data->out_flags  |= PF_OutFlag_DEEP_COLOR_AWARE;
    // PF_OutFlag2_I_AM_THREADSAFE: legacy (SDK で "unused" とされる互換シグナル、維持)
    // PF_OutFlag2_SUPPORTS_THREADED_RENDERING (bit 27 = 0x08000000):
    //   Multi-Frame Rendering 対応。Render セレクタが複数スレッドから同時に呼ばれる。
    //   Pipl.r::AE_Effect_Global_OutFlags_2 と**常に同期**すること(同期忘れは effect
    //   が legacy 扱いになるだけで AE はエラーを出さないので気付きにくい)。
    // PF_OutFlag2_SUPPORTS_GET_FLATTENED_SEQUENCE_DATA (bit 23 = 0x00800000):
    //   SDK doc は「SEQUENCE_DATA_NEEDS_FLATTENING と THREADED_RENDERING の両方が
    //   立つ時に必須」と書かれているが、AE 2025 の FLTp_EnforceFlagCombinations は
    //   legacy render (PF_Cmd_RENDER) 経路の MFR 対応 plugin 全般に要求してくる
    //   (sequence_data 未使用でも plugin scan / project load 時に verification
    //   failure の error dialog が出る)。本 plugin は sequence_data を使っていない
    //   ため、PF_Cmd_GET_FLATTENED_SEQUENCE_DATA ハンドラは未実装 = AE が NULL を
    //   受けて問題なし。
    // PF_OutFlag2_SUPPORTS_SMART_RENDER (bit 10 = 0x400):
    //   Phase 2-A.1 で追加。AE が SMART_PRE_RENDER / SMART_RENDER 経路で
    //   plugin を呼ぶようになる。legacy PF_Cmd_RENDER は後方互換のため残置。
    // PF_OutFlag2_FLOAT_COLOR_AWARE (bit 12 = 0x1000):
    //   Phase 2-A.2 で追加。32bpc(PF_PixelFloat、ARGB128)プロジェクトでも
    //   AE がこの effect に PF_Cmd_SMART_RENDER を発行するようになる。
    //   未指定時は Composition 32bpc で「effect が黄色三角」となり 8/16bpc
    //   へのダウングレード or skip となる。SmartRender / Render は内部で
    //   PF_GetPixelFormat による format 分岐を行い、ARGB128 は
    //   smoothing<PF_PixelFloat, KP_PIXEL128>() に dispatch する。
    // PF_OutFlag2_SUPPORTS_GPU_RENDER_F32 (bit 25 = 0x02000000):
    //   Phase 2-A.3 Sub-stage C-2 で追加。AE が PF_Cmd_GPU_DEVICE_SETUP /
    //   SMART_RENDER_GPU を発行可能にする。AE_Effect.h L1007 の通り、
    //   この flag は **3 箇所**(GlobalSetup の out_flags2 / Pipl.r /
    //   GPU_DEVICE_SETUP の out_data->out_flags2)に立てる必要がある。
    //   GlobalSetup は plugin-level、GPU_DEVICE_SETUP は per-device-level
    //   の宣言で、どちらか欠けると AE は GPU 経路を skip する(エラーは
    //   出さず無音で CPU SmartRender に倒すので発見が遅れる)。
    //   Pipl.r::AE_Effect_Global_OutFlags_2 と**常に同期**(現値 0x0A801410)。
    out_data->out_flags2 |= PF_OutFlag2_I_AM_THREADSAFE
                          | PF_OutFlag2_SUPPORTS_THREADED_RENDERING
                          | PF_OutFlag2_SUPPORTS_GET_FLATTENED_SEQUENCE_DATA
                          | PF_OutFlag2_SUPPORTS_SMART_RENDER
                          | PF_OutFlag2_FLOAT_COLOR_AWARE
                          | PF_OutFlag2_SUPPORTS_GPU_RENDER_F32;

    // Phase 2-A.3 Sub-stage C-2: backend usability seed.
    // Sub-stage D wires this to the §4.3 detection result(`GetDeviceCount`
    // + OS API)。C-2 では「とりあえず true」で進め、PreRender 5-condition
    // AND の (d) を満たす状態を作る。Sub-stage D で本実装に置換するまで
    // GPU 非対応環境でも GPU_RENDER_POSSIBLE が立ってしまうが、AE 側が
    // GPU_DEVICE_SETUP を呼ばない構成では SMART_RENDER_GPU 自体が来ない
    // ので落ちない(条件 (e) が事実上ガード)。詳細は RFC §3.3.1 / §4.3。
    smooth_core_gpu_set_backend_usable(1);

    return PF_Err_NONE;
}



static PF_Err GlobalSetdown(PF_InData       *in_data,
                            PF_OutData      *out_data,
                            PF_ParamDef     *params[],
                            PF_LayerDef     *output )
{	
    return PF_Err_NONE;
}


//---------------------------------------------------------------------------//
// 概要   : パラメータの設定
// 関数名 : ParamsSetup
// 引数   : 
// 返り値 : 
//---------------------------------------------------------------------------//
static PF_Err ParamsSetup(  PF_InData       *in_data,
                            PF_OutData      *out_data,
                            PF_ParamDef     *params[],
                            PF_LayerDef     *output)
{
    
    PF_ParamDef def;
    AEFX_CLR_STRUCT(def);   // defを初期化 //

    def.param_type = PF_Param_CHECKBOX;
    def.flags = PF_ParamFlag_START_COLLAPSED;
    PF_STRCPY(def.PF_DEF_NAME, "white option");
    def.u.bd.value = def.u.bd.dephault = FALSE;
    def.u.bd.u.nameptr = "transparent"; /* this is strictly a pointer; don't STRCPY into it! */
    
    PF_ADD_PARAM(in_data, -1, &def);

    AEFX_CLR_STRUCT(def);


    PF_ADD_FLOAT_SLIDER("range",
                        0.0f,           //VALID_MIN,
                        100.0f,         //VALID_MAX,
                        0.0f,           //SLIDER_MIN,
                        10.0f,          //SLIDER_MAX,
                        1.00f,          //CURVE_TOLERANCE,  よくわかんない
                        1.0f,           //DFLT, デフォルト
                        1,              //DISP  会いたいをそのまま表示
                        0,              //PREC, パーセント表示？,
                        FALSE,          //WANT_PHASE,
                        PARAM_RANGE);   // ID

    PF_ADD_FLOAT_SLIDER("line weight",
                        0.0f,           //VALID_MIN,
                        1.0f,           //VALID_MAX,
                        0.0f,           //SLIDER_MIN,
                        1.0f,           //SLIDER_MAX,
                        1.0f,           //CURVE_TOLERANCE,  よくわかんない
                        0.0f,           //DFLT, デフォルト
                        1,              //DISP  会いたいをそのまま表示
                        0,              //PREC, パーセント表示？,
                        FALSE,          //WANT_PHASE,
                        PARAM_LINE_WEIGHT ); // ID

    // Build 表示(読み取り専用)。
    // Rust staticlib からビルド識別子("0.1.0+<sha>[+dirty]")を取得し、
    // Effect Controls パネルに静的表示する。偽成功(incremental cache による
    // Phase 1 相当バイナリ)をユーザーがひと目で判別できるようにするのが目的。
    // ボタンとして表示され、クリック時には PF_Cmd_USER_CHANGED_PARAM 経由で
    // About ダイアログを呼び出して詳細情報を出す(EntryPointFunc 参照)。
    // PF_ParamFlag_SUPERVISE を立てないとクリックが届かない(AE_Effect.h L480)。
    AEFX_CLR_STRUCT(def);
    def.param_type = PF_Param_BUTTON;
    def.flags      = PF_ParamFlag_SUPERVISE | PF_ParamFlag_CANNOT_TIME_VARY | PF_ParamFlag_CANNOT_INTERP;
    PF_STRCPY(def.PF_DEF_NAME, "Build");
    def.u.button_d.u.namesptr = smooth_core_build_id();  // static C string; lifetime OK
    PF_ADD_PARAM(in_data, -1, &def);

    // Phase 2-A.3 Sub-stage C-2: GPU Acceleration checkbox.
    //   default ON    — turn the GPU path on for any project that has the rest
    //                   of the §3.3.1 5-condition AND satisfied. Users can opt
    //                   out per-effect-instance to force CPU.
    //   32bpc only    — even with this ON, 8/16bpc projects stay on CPU
    //                   SmartRender (PreRender condition (a)). The checkbox
    //                   has no visible effect on those.
    //   DISABLED wiring (Sub-stage D): when GLOBAL_SETUP detects no usable GPU
    //                   backend (no Metal device on Mac, no NVIDIA driver on
    //                   Win), this param will register with PF_ParamFlag_DISABLED
    //                   so the checkbox greys out. C-2 always registers it
    //                   enabled; the static disable is layered in D once §4.3
    //                   detection is wired.
    AEFX_CLR_STRUCT(def);
    def.param_type = PF_Param_CHECKBOX;
    def.flags      = PF_ParamFlag_SUPERVISE | PF_ParamFlag_START_COLLAPSED;
    PF_STRCPY(def.PF_DEF_NAME, "GPU Acceleration");
    def.u.bd.value = def.u.bd.dephault = TRUE;
    def.u.bd.u.nameptr = "GPU Acceleration (32bpc only)";
    PF_ADD_PARAM(in_data, -1, &def);

    // パラメータ数をセット //
    out_data->num_params = PARAM_NUM;


    return PF_Err_NONE;
}





//---------------------------------------------------------------------------//
// smoothing実行関数 
// PixelType		PF_Pixel8, PF_Pixel16
// PackedPixelType	KP_PIXEL32,	KP_PIXEL64
//---------------------------------------------------------------------------//
template<typename PixelType, typename PackedPixelType>
static PF_Err smoothing(PF_InData               *in_data,
						PF_OutData              *out_data,
                        const SmartRenderInfo   *info,
						PF_LayerDef             *input,
						PF_LayerDef             *output,
						PixelType	            *in_ptr,
						PixelType	            *out_ptr)
{
	PF_Err	err = PF_Err_NONE;

    BEGIN_PROFILE();
    SMOOTH_BENCH_TIMER_BEGIN();

    // smooth_core::process() の契約は「in_ptr / out_ptr は独立した writable
    // バッファ」かつ「preProcess が in_ptr を in-place 改変する」。AE 2025 の
    // SmartRender 経路では PF_OutFlag_I_WRITE_INPUT_BUFFER が許可されない
    // ため、AE 提供の input buffer は read-only として扱う必要がある。
    // ここで scratch を確保して input pixels をコピーし、smooth_core には
    // scratch を in_ptr として渡す。出費は rowbytes*height bytes / call。
    const size_t scratch_bytes = (size_t)input->rowbytes * (size_t)input->height;
    PixelType *scratch = (PixelType*)malloc(scratch_bytes);
    if (!scratch) return PF_Err_OUT_OF_MEMORY;
    memcpy(scratch, in_ptr, scratch_bytes);

    // パラメータを core 形式へ変換(SmartRenderInfo の raw slider 値ベース)。
    // 8/16bpc は max_value=0xFF/0x8000 の整数 domain で u32 sum しきい値を渡し、
    // 32bpc は max_value=1.0 の f32 domain で f32 sum しきい値を渡す。
    // Phase 2-A.2 Step 2 で f32 path を追加(Params::range_f32)。
    smooth_core::Params core_params;
    if constexpr (sizeof(PixelType) == 16) {
        core_params.range     = 0;  // unused on 32bpc path
        core_params.range_f32 = (float)(info->range * 4.0 / 100.0);  // max=1.0, 4 channels
    } else {
        core_params.range     = (unsigned int)(info->range * (getMaxValue<PixelType>() * 4)) / 100;
        core_params.range_f32 = 0.0f;
    }
    core_params.line_weight  = (float)(info->line_weight / 2.0 + 0.5);
    core_params.white_option = info->white_option;

    // AE SDK 非依存のコア処理を呼ぶ(scratch を in、out_ptr を out)
    smooth_core::process<PixelType>(scratch, out_ptr,
                                    input->width, input->height, input->rowbytes,
                                    core_params);



    END_PROFILE();

    SMOOTH_BENCH_CAPTURE(
        GET_WIDTH(input),
        GET_HEIGHT(input),
        (int)(sizeof(PixelType) * 8 / 4),           // bpc: Pixel8->8, Pixel16->16, PF_PixelFloat->32
        input->rowbytes,
        scratch,                                    // pre/post-preProcess snapshot は scratch 側
        out_ptr,
        core_params.range,                          // bench は u32 range のみ記録(32bpc は 0)
        core_params.line_weight,
        info->white_option ? 1 : 0);

    free(scratch);
	return err;
}

// PARAM_RANGE / PARAM_LINE_WEIGHT / PARAM_WHITE_OPTION / PARAM_GPU_ACCELERATION を
// SmartRenderInfo に snapshot するユーティリティ。Render(legacy)は params[] から、
// SmartRender は pre_render_data 経由で値を取る形に統一する。
static inline void params_to_smart_info(PF_ParamDef *params[], SmartRenderInfo *info)
{
    info->range            = params[PARAM_RANGE]->u.fs_d.value;
    info->line_weight      = params[PARAM_LINE_WEIGHT]->u.fs_d.value;
    info->white_option     = params[PARAM_WHITE_OPTION]->u.bd.value ? true : false;
    info->gpu_acceleration = params[PARAM_GPU_ACCELERATION]->u.bd.value ? true : false;
}

// Phase 2-A.3 Sub-stage C-2: read the per-instance UUID out of sequence_data.
// The handle is allocated by SEQUENCE_SETUP / SEQUENCE_RESETUP. If
// in_data->sequence_data is null (e.g., a legacy project loaded for the first
// time after this build, before AE has called RESETUP), returns false and
// leaves *out_lo / *out_hi at zero — callers treat that as "no UUID yet,
// skip the fallen check" (PreRender's 5-condition (c) is a NEGATIVE check, so
// a missing UUID just means "definitely not fallen").
static inline bool read_sequence_uuid(PF_InData *in_data,
                                      uint64_t *out_lo, uint64_t *out_hi)
{
    *out_lo = 0;
    *out_hi = 0;
    PF_Handle h = (PF_Handle)in_data->sequence_data;
    if (!h) return false;
    SequenceData *sd = (SequenceData*)PF_LOCK_HANDLE(h);
    if (!sd) return false;
    *out_lo = sd->uuid_lo;
    *out_hi = sd->uuid_hi;
    PF_UNLOCK_HANDLE(h);
    return true;
}

// Phase 2-A.2 Step 2: PF_GET_PIXEL_DATA macros には 32bpc 版が無いため、
// PF_WorldSuite2::PF_GetPixelFormat で format を取得して 8/16/32bpc を分岐する。
// suite が取得できない or 取得失敗時は INVALID を返し、呼び出し側は legacy
// pixel pointer の null チェック(8 vs 16)へ fallback する。
static inline PF_PixelFormat detect_pixel_format(PF_InData *in_data, PF_EffectWorld *world)
{
    if (!in_data || !in_data->pica_basicP || !world) return PF_PixelFormat_INVALID;
    PF_WorldSuite2 *wsP = NULL;
    PF_PixelFormat fmt  = PF_PixelFormat_INVALID;
    if (in_data->pica_basicP->AcquireSuite(kPFWorldSuite, kPFWorldSuiteVersion2,
                                           (const void**)&wsP) == 0 && wsP) {
        wsP->PF_GetPixelFormat(world, &fmt);
        in_data->pica_basicP->ReleaseSuite(kPFWorldSuite, kPFWorldSuiteVersion2);
    }
    return fmt;
}


//---------------------------------------------------------------------------//
// 概要   : レンダリング
// 関数名 : Render
// 引数   : 
// 返り値 : 
//---------------------------------------------------------------------------//
static PF_Err Render (  PF_InData       *in_data,
                        PF_OutData      *out_data,
                        PF_ParamDef     *params[],
                        PF_LayerDef     *output )
{
    PF_Err err = PF_Err_NONE;

	PF_LayerDef *input  = &params[PARAM_INPUT]->u.ld;

    SmartRenderInfo info;
    params_to_smart_info(params, &info);

    // Phase 2-A.2 Step 2: 32bpc(PF_PixelFloat / ARGB128)を含む 3 段分岐。
    // 既存の "PF_GET_PIXEL_DATA16 が NULL なら 8bpc" 推定では 32bpc を 8bpc と
    // 誤判定してしまうため、PF_GetPixelFormat で明示的に判別する。
    const PF_PixelFormat fmt = detect_pixel_format(in_data, input);
    if (fmt == PF_PixelFormat_ARGB128) {
        err = smoothing<PF_PixelFloat, KP_PIXEL128>(in_data, out_data, &info,
                                                    input, output,
                                                    (PF_PixelFloat*)input->data,
                                                    (PF_PixelFloat*)output->data);
        return err;
    }

	PF_Pixel16	*in_ptr16, *out_ptr16;
	PF_GET_PIXEL_DATA16(output, NULL, &out_ptr16 );
	PF_GET_PIXEL_DATA16(input, NULL, &in_ptr16 );

	if( out_ptr16 != NULL && in_ptr16 != NULL )
	{
		// 16bpc
		err = smoothing<PF_Pixel16, KP_PIXEL64>(in_data, out_data, &info,
												input, output, in_ptr16, out_ptr16 );
	}
	else
	{
		// 8bpc
		PF_Pixel8	*in_ptr8, *out_ptr8;
		PF_GET_PIXEL_DATA8(output, NULL, &out_ptr8 );
		PF_GET_PIXEL_DATA8(input, NULL, &in_ptr8 );

		err = smoothing<PF_Pixel8, KP_PIXEL32>(in_data, out_data, &info,
												input, output, in_ptr8, out_ptr8 );
	}

	return err;


}


//---------------------------------------------------------------------------//
// Phase 2-A.1: SmartRender(CPU 経路)
//
// SmartRender は Render と違い 2 段階(PreRender → Render)。PreRender 時点で
// (a) 非 layer params を checkout して pre_render_data に snapshot 保存
// (b) input layer を checkout して result_rect / max_result_rect を返却
// SmartRender 時に pre_render_data の snapshot + checkout 済 input/output
// world を使って既存の smoothing<>() を呼ぶ。
//
// GPU 経路(PF_Cmd_SMART_RENDER_GPU)は Phase 2-A.3 で追加、PreRender で
// PF_RenderOutputFlag_GPU_RENDER_POSSIBLE を立てる必要がある。Step 1 では
// 立てない(CPU 専用、AE は SMART_RENDER だけ呼んでくる)。
//---------------------------------------------------------------------------//

// Smart_Utils.cpp::UnionLRect 同等。SDK util が build に含まれていないので
// inline 化(空 rect を考慮した両端の min/max 取り)。
static inline void union_lrect_inline(const PF_LRect *src, PF_LRect *dst)
{
    if (dst->left == dst->right || dst->top == dst->bottom) {
        *dst = *src;
        return;
    }
    if (src->left == src->right || src->top == src->bottom) return;
    if (src->left   < dst->left)   dst->left   = src->left;
    if (src->top    < dst->top)    dst->top    = src->top;
    if (src->right  > dst->right)  dst->right  = src->right;
    if (src->bottom > dst->bottom) dst->bottom = src->bottom;
}

static void DisposeSmartRenderInfo(void *infoPV)
{
    if (infoPV) free(infoPV);
}

static PF_Err SmartPreRender(PF_InData         *in_data,
                             PF_OutData        *out_data,
                             PF_PreRenderExtra *extraP)
{
    PF_Err err = PF_Err_NONE;
    PF_CheckoutResult in_result;
    PF_RenderRequest req = extraP->input->output_request;

    SmartRenderInfo *info = (SmartRenderInfo*)malloc(sizeof(SmartRenderInfo));
    if (!info) return PF_Err_OUT_OF_MEMORY;

    PF_ParamDef cur_param;

    err = PF_CHECKOUT_PARAM(in_data, PARAM_RANGE, in_data->current_time,
                            in_data->time_step, in_data->time_scale, &cur_param);
    if (err) { free(info); return err; }
    info->range = cur_param.u.fs_d.value;

    err = PF_CHECKOUT_PARAM(in_data, PARAM_LINE_WEIGHT, in_data->current_time,
                            in_data->time_step, in_data->time_scale, &cur_param);
    if (err) { free(info); return err; }
    info->line_weight = cur_param.u.fs_d.value;

    err = PF_CHECKOUT_PARAM(in_data, PARAM_WHITE_OPTION, in_data->current_time,
                            in_data->time_step, in_data->time_scale, &cur_param);
    if (err) { free(info); return err; }
    info->white_option = cur_param.u.bd.value ? true : false;

    err = PF_CHECKOUT_PARAM(in_data, PARAM_GPU_ACCELERATION, in_data->current_time,
                            in_data->time_step, in_data->time_scale, &cur_param);
    if (err) { free(info); return err; }
    info->gpu_acceleration = cur_param.u.bd.value ? true : false;

    extraP->output->pre_render_data = info;
    extraP->output->delete_pre_render_data_func = DisposeSmartRenderInfo;

    err = extraP->cb->checkout_layer(in_data->effect_ref,
                                     PARAM_INPUT, PARAM_INPUT,
                                     &req,
                                     in_data->current_time,
                                     in_data->time_step,
                                     in_data->time_scale,
                                     &in_result);
    if (err) return err;

    union_lrect_inline(&in_result.result_rect,     &extraP->output->result_rect);
    union_lrect_inline(&in_result.max_result_rect, &extraP->output->max_result_rect);

    // Phase 2-A.3 Sub-stage C-2: 5-condition AND for GPU_RENDER_POSSIBLE.
    // Per RFC §3.3.1, all five must hold for AE to be allowed to call
    // SMART_RENDER_GPU on this frame:
    //   (a) input is 32bpc (PF_PixelFloat / ARGB128)        — extraP->input->bitdepth
    //   (b) GPU Acceleration checkbox is ON                  — info->gpu_acceleration
    //   (c) this instance has NOT been marked fallen         — smooth_core_gpu_is_fallen
    //   (d) plugin-global backend is usable                  — smooth_core_gpu_is_backend_usable
    //   (e) GPU_DEVICE_SETUP succeeded                       — proxied via (d) in C-2;
    //                                                          Sub-stage D splits these.
    // (c) requires the per-instance UUID. read_sequence_uuid returns false if
    // the sequence_data handle is null (early in a fresh effect application,
    // or a legacy project that has not yet been touched by RESETUP). In that
    // case we treat "no UUID" as "definitely not fallen" — there is no entry
    // in GPU_FALLEN to be set yet.
    const bool cond_a = (extraP->input->bitdepth == 32);
    const bool cond_b = info->gpu_acceleration;
    bool       cond_c = true;  // default if no UUID yet
    uint64_t uuid_lo, uuid_hi;
    if (read_sequence_uuid(in_data, &uuid_lo, &uuid_hi)) {
        cond_c = (smooth_core_gpu_is_fallen(uuid_lo, uuid_hi) == 0);
    }
    const bool cond_d = (smooth_core_gpu_is_backend_usable() != 0);
    const bool cond_e = cond_d;  // C-2 stub: merged with (d) until Sub-stage D

    const bool all_conditions = (cond_a && cond_b && cond_c && cond_d && cond_e);
#if SMOOTH_GPU_DISPATCH_READY
    if (all_conditions) {
        extraP->output->flags |= PF_RenderOutputFlag_GPU_RENDER_POSSIBLE;
    }
#else
    // C-2 stub: 5-condition AND is computed for verification (a debug build
    // could log `all_conditions` here), but the flag is intentionally not
    // raised so AE never dispatches SMART_RENDER_GPU. See
    // SMOOTH_GPU_DISPATCH_READY comment above SequenceData.
    (void)all_conditions;
#endif

    return PF_Err_NONE;
}

static PF_Err SmartRender(PF_InData            *in_data,
                          PF_OutData           *out_data,
                          PF_SmartRenderExtra  *extraP)
{
    PF_Err err = PF_Err_NONE;
    PF_EffectWorld *input_world  = NULL;
    PF_EffectWorld *output_world = NULL;

    SmartRenderInfo *info = (SmartRenderInfo*)extraP->input->pre_render_data;
    if (!info) return PF_Err_INTERNAL_STRUCT_DAMAGED;

    err = extraP->cb->checkout_layer_pixels(in_data->effect_ref, PARAM_INPUT, &input_world);
    if (err || !input_world) return err ? err : PF_Err_INTERNAL_STRUCT_DAMAGED;

    err = extraP->cb->checkout_output(in_data->effect_ref, &output_world);
    if (err || !output_world) {
        extraP->cb->checkin_layer_pixels(in_data->effect_ref, PARAM_INPUT);
        return err ? err : PF_Err_INTERNAL_STRUCT_DAMAGED;
    }

    // Phase 2-A.2 Step 2: bpc 判定は PF_GetPixelFormat 結果を優先。
    // PF_GET_PIXEL_DATA16 / DATA8 は 32bpc 時に両方 NULL を返してしまうため、
    // ARGB128 を最初にハンドルしてから 16/8bpc に分岐する。
    const PF_PixelFormat fmt = detect_pixel_format(in_data, input_world);
    if (fmt == PF_PixelFormat_ARGB128) {
        err = smoothing<PF_PixelFloat, KP_PIXEL128>(in_data, out_data, info,
                                                    input_world, output_world,
                                                    (PF_PixelFloat*)input_world->data,
                                                    (PF_PixelFloat*)output_world->data);
    } else {
        PF_Pixel16 *in_ptr16, *out_ptr16;
        PF_GET_PIXEL_DATA16(output_world, NULL, &out_ptr16);
        PF_GET_PIXEL_DATA16(input_world,  NULL, &in_ptr16);

        if (out_ptr16 != NULL && in_ptr16 != NULL) {
            err = smoothing<PF_Pixel16, KP_PIXEL64>(in_data, out_data, info,
                                                    input_world, output_world,
                                                    in_ptr16, out_ptr16);
        } else {
            PF_Pixel8 *in_ptr8, *out_ptr8;
            PF_GET_PIXEL_DATA8(output_world, NULL, &out_ptr8);
            PF_GET_PIXEL_DATA8(input_world,  NULL, &in_ptr8);
            err = smoothing<PF_Pixel8, KP_PIXEL32>(in_data, out_data, info,
                                                   input_world, output_world,
                                                   in_ptr8, out_ptr8);
        }
    }

    extraP->cb->checkin_layer_pixels(in_data->effect_ref, PARAM_INPUT);
    return err;
}








//---------------------------------------------------------------------------//
// Phase 2-A.3 Sub-stage C-2: sequence_data + GPU lifecycle handlers
//
// SequenceSetup     : alloc PF_Handle, generate UUID via Rust FFI
// SequenceResetup   : regenerate UUID (RFC §6.5: never carry across save/load)
// SequenceFlatten   : no-op — our SequenceData is plain old data, no
//                     pointers / no platform handles, intrinsically flat
// SequenceSetdown   : forget UUID (cleans GPU_FALLEN entry), dispose handle
// GetFlattenedSeq.  : return a copy of the current handle (already flat)
// GpuDeviceSetup    : set out_data->out_flags2 |= SUPPORTS_GPU_RENDER_F32
//                     (the third place this flag is required, see
//                     GlobalSetup comment)
// GpuDeviceSetdown  : no-op for C-2 (nothing allocated per-device yet);
//                     C-2.5 will dispose Metal command queues / pipelines
//                     here when those are created in DEVICE_SETUP.
//---------------------------------------------------------------------------//

static PF_Err SequenceSetup(PF_InData *in_data, PF_OutData *out_data)
{
    PF_Handle h = (*in_data->utils->host_new_handle)(sizeof(SequenceData));
    if (!h) return PF_Err_OUT_OF_MEMORY;
    SequenceData *sd = (SequenceData*)PF_LOCK_HANDLE(h);
    if (!sd) { PF_DISPOSE_HANDLE(h); return PF_Err_OUT_OF_MEMORY; }
    smooth_core_gpu_uuid_new(&sd->uuid_lo, &sd->uuid_hi);
    PF_UNLOCK_HANDLE(h);
    out_data->sequence_data = (PF_Handle)h;
    return PF_Err_NONE;
}

static PF_Err SequenceResetup(PF_InData *in_data, PF_OutData *out_data)
{
    PF_Handle h = (PF_Handle)in_data->sequence_data;
    if (!h) {
        // Legacy projects (saved without sequence_data) reach RESETUP with a
        // null handle. Allocate fresh — same as SETUP.
        return SequenceSetup(in_data, out_data);
    }
    SequenceData *sd = (SequenceData*)PF_LOCK_HANDLE(h);
    if (!sd) return PF_Err_INTERNAL_STRUCT_DAMAGED;
    // RFC §6.5: regenerate the UUID at every RESETUP. Any GPU_FALLEN entry
    // keyed by the OLD uuid is left to expire on its own — DashMap entries
    // are removed only at SETDOWN; in the meantime the old key just goes
    // unreferenced (memory cost is one (u128, AtomicBool) pair, negligible).
    smooth_core_gpu_uuid_new(&sd->uuid_lo, &sd->uuid_hi);
    PF_UNLOCK_HANDLE(h);
    out_data->sequence_data = (PF_Handle)h;  // pass through, AE owns lifetime
    return PF_Err_NONE;
}

static PF_Err SequenceFlatten(PF_InData *in_data, PF_OutData *out_data)
{
    // SequenceData has no pointers / no platform handles, so it is already
    // flat. AE may still call this selector on plugins that opt into MFR;
    // returning success with no-op is the documented expectation for plain-
    // old-data sequence_data.
    (void)in_data; (void)out_data;
    return PF_Err_NONE;
}

static PF_Err SequenceSetdown(PF_InData *in_data, PF_OutData *out_data)
{
    PF_Handle h = (PF_Handle)in_data->sequence_data;
    if (h) {
        SequenceData *sd = (SequenceData*)PF_LOCK_HANDLE(h);
        if (sd) {
            // Cleanup: drop the GPU_FALLEN entry so a future SETUP starts
            // with a clean slate (RFC §3.3.1 once-fallen-always-fall scope is
            // SETUP/RESETUP span only, not effect-instance lifetime).
            smooth_core_gpu_forget(sd->uuid_lo, sd->uuid_hi);
            PF_UNLOCK_HANDLE(h);
        }
        PF_DISPOSE_HANDLE(h);
        out_data->sequence_data = NULL;
    }
    return PF_Err_NONE;
}

static PF_Err GetFlattenedSequenceData(PF_InData *in_data, PF_OutData *out_data)
{
    PF_Handle src_h = (PF_Handle)in_data->sequence_data;
    if (!src_h) {
        // No sequence_data to flatten — leave out_data->sequence_data null.
        return PF_Err_NONE;
    }
    PF_Handle dst_h = (*in_data->utils->host_new_handle)(sizeof(SequenceData));
    if (!dst_h) return PF_Err_OUT_OF_MEMORY;
    SequenceData *src = (SequenceData*)PF_LOCK_HANDLE(src_h);
    SequenceData *dst = (SequenceData*)PF_LOCK_HANDLE(dst_h);
    if (src && dst) {
        *dst = *src;
    }
    if (src) PF_UNLOCK_HANDLE(src_h);
    if (dst) PF_UNLOCK_HANDLE(dst_h);
    out_data->sequence_data = (PF_Handle)dst_h;
    return PF_Err_NONE;
}

static PF_Err GpuDeviceSetup(PF_InData *in_data, PF_OutData *out_data, void *extra)
{
    PF_Err err = PF_Err_NONE;
    PF_GPUDeviceSetupExtra *gpu_extra = (PF_GPUDeviceSetupExtra*)extra;

    // §4.4 fault injection: simulate "device setup failed" via env var.
    if (smooth_core_gpu_should_force_error(1) /* "setup" */) {
        return PF_Err_OUT_OF_MEMORY;
    }

    // Mac: we only accept Metal devices. AE may also offer OpenCL on older
    // hosts; skipping by leaving SUPPORTS_GPU_RENDER_F32 unset tells AE not
    // to call SMART_RENDER_GPU on this device. Win CUDA path lands in
    // Sub-stage E with a parallel branch.
#ifdef __APPLE__
    if (!gpu_extra || !gpu_extra->input ||
        gpu_extra->input->what_gpu != PF_GPU_Framework_METAL) {
        return PF_Err_NONE;  // not Metal — don't accept
    }

    // Acquire the GPU device suite to get MTLDevice / MTLCommandQueue raw
    // pointers for the offered device_index.
    PF_GPUDeviceSuite1 *gpu_suite = NULL;
    if (in_data->pica_basicP->AcquireSuite(kPFGPUDeviceSuite, kPFGPUDeviceSuiteVersion1,
                                           (const void**)&gpu_suite) != 0 || !gpu_suite) {
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
    }
    PF_GPUDeviceInfo info = {};
    err = gpu_suite->GetDeviceInfo(in_data->effect_ref,
                                   gpu_extra->input->device_index, &info);
    if (err) {
        in_data->pica_basicP->ReleaseSuite(kPFGPUDeviceSuite, kPFGPUDeviceSuiteVersion1);
        return err;
    }
    if (info.device_framework != PF_GPU_Framework_METAL || !info.devicePV || !info.command_queuePV) {
        in_data->pica_basicP->ReleaseSuite(kPFGPUDeviceSuite, kPFGPUDeviceSuiteVersion1);
        return PF_Err_NONE;  // unsupported / missing — don't accept
    }
    if (!info.compatibleB) {
        // Driver says this device cannot accelerate; honour it.
        in_data->pica_basicP->ReleaseSuite(kPFGPUDeviceSuite, kPFGPUDeviceSuiteVersion1);
        return PF_Err_NONE;
    }

    // Build the Rust-side MetalBackend (compiles MSL, builds compute pipeline).
    void *metal_handle = smooth_core_metal_create(info.devicePV, info.command_queuePV);
    in_data->pica_basicP->ReleaseSuite(kPFGPUDeviceSuite, kPFGPUDeviceSuiteVersion1);
    if (!metal_handle) {
        // MSL compile / pipeline build failed for this device. Decline politely.
        return PF_Err_NONE;
    }

    // Stash the handle so AE round-trips it back via PF_SmartRenderInput->gpu_data.
    if (gpu_extra->output) {
        gpu_extra->output->gpu_data = metal_handle;
    } else {
        // Defensive: no output struct means AE can't carry our handle. Free it.
        smooth_core_metal_destroy(metal_handle);
        return PF_Err_INTERNAL_STRUCT_DAMAGED;
    }

    // Required: 3rd of 3 sites for SUPPORTS_GPU_RENDER_F32 (the other two are
    // GlobalSetup out_flags2 and Pipl.r). Without this, AE silently routes to
    // CPU SmartRender and never calls SMART_RENDER_GPU on this device.
    out_data->out_flags2 |= PF_OutFlag2_SUPPORTS_GPU_RENDER_F32;
    return PF_Err_NONE;
#else
    (void)gpu_extra;
    // Non-Apple builds (Win Sub-stage E) reach here with what_gpu == CUDA.
    // C-2.5a is Mac-only; Sub-stage E will add the parallel CUDA branch
    // and OR the flag from there.
    return PF_Err_NONE;
#endif
}

static PF_Err GpuDeviceSetdown(PF_InData *in_data, PF_OutData *out_data, void *extra)
{
    (void)in_data; (void)out_data;
#ifdef __APPLE__
    PF_GPUDeviceSetdownExtra *gpu_extra = (PF_GPUDeviceSetdownExtra*)extra;
    if (gpu_extra && gpu_extra->input && gpu_extra->input->gpu_data) {
        smooth_core_metal_destroy(gpu_extra->input->gpu_data);
        // The SDK header notes: "effect must dispose"; setting the field
        // back to NULL is good hygiene even though AE will not read it
        // again on this code path.
        gpu_extra->input->gpu_data = NULL;
    }
#else
    (void)extra;
#endif
    return PF_Err_NONE;
}

// Phase 2-A.3 Sub-stage C-2: GPU SmartRender stub.
//
// Real Metal kernel dispatch is C-2.5. For C-2 we only verify the plumbing:
// AE calls us when the 5 conditions held in PreRender, the input/output GPU
// worlds get checked out, fault injection can fire, and on success we
// produce visible output (a CPU fallback path keeps the comp rendering even
// while the shader is identity-only). The fallback is the device-host-device
// (RFC §4.4 採用 (i)) shape we want anyway: any future GPU error path will
// reuse this same code.
// Mark this instance fallen and return PF_Err_NONE so AE's Render Queue
// keeps moving (RFC §4.4 採用 (i): device->host->device or equivalent
// graceful fallback). The next PreRender for this instance will see the
// fallen flag and skip GPU entirely, routing to CPU SmartRender.
static inline PF_Err mark_fallen_and_continue(PF_InData *in_data)
{
    uint64_t uuid_lo, uuid_hi;
    if (read_sequence_uuid(in_data, &uuid_lo, &uuid_hi)) {
        smooth_core_gpu_mark_fallen(uuid_lo, uuid_hi);
    }
    return PF_Err_NONE;
}

// Best-effort identity copy from src to dst on the GPU. Used by
// SmartRenderGpu's error paths so the AE-allocated dst buffer is never
// returned blank — AE shows a "smooth did not render anything" warning
// when an effect dispatches SMART_RENDER_GPU but its output buffer ends
// up unwritten. For us, "didn't reach the smooth chain" should fall back
// to "passthrough what we got" rather than "leave the buffer blank".
//
// Returns true when the passthrough kernel was successfully submitted,
// false when the inputs were not viable (any null pointer, zero extent,
// or kernel rc != 0 — though even an rc != 0 means the kernel was queued
// in some form, so we report success for safety).
#ifdef __APPLE__
static inline bool gpu_passthrough_to_dst(
    void *metal_handle, void *src_buf, void *dst_buf,
    uint32_t src_pitch_pixels, uint32_t dst_pitch_pixels,
    uint32_t width, uint32_t height)
{
    if (!metal_handle || !src_buf || !dst_buf || width == 0 || height == 0) return false;
    (void)smooth_core_metal_dispatch_passthrough(
        metal_handle, src_buf, dst_buf,
        src_pitch_pixels, dst_pitch_pixels, width, height);
    return true;
}
#endif

static PF_Err SmartRenderGpu(PF_InData            *in_data,
                             PF_OutData           *out_data,
                             PF_SmartRenderExtra  *extraP)
{
#if !SMOOTH_GPU_DISPATCH_READY
    // Defensive guard. PreRender does not raise GPU_RENDER_POSSIBLE while
    // this gate is 0, so AE should never reach here. If it does (a stale
    // cached PreRender result, a future AE / driver behaviour change), mark
    // fallen and bail out without touching the GPU world.
    (void)out_data; (void)extraP;
    return mark_fallen_and_continue(in_data);
#else
    PF_Err err = PF_Err_NONE;
    (void)out_data;

#ifdef __APPLE__
    // The MetalBackend handle was stashed by GpuDeviceSetup and AE round-
    // trips it back to us via PF_SmartRenderInput->gpu_data.
    void *metal_handle = (void*)extraP->input->gpu_data;
    if (!metal_handle) return mark_fallen_and_continue(in_data);

    // Mac builds only accept Metal devices in GpuDeviceSetup, so what_gpu
    // here should always be METAL. Defensive check: bail out if AE somehow
    // routed a non-Metal device here.
    if (extraP->input->what_gpu != PF_GPU_Framework_METAL) {
        return mark_fallen_and_continue(in_data);
    }

    // Acquire the GPU suite to (a) check out the input/output GPU effect
    // worlds and (b) translate them to MTLBuffer raw pointers.
    PF_GPUDeviceSuite1 *gpu_suite = NULL;
    if (in_data->pica_basicP->AcquireSuite(kPFGPUDeviceSuite, kPFGPUDeviceSuiteVersion1,
                                           (const void**)&gpu_suite) != 0 || !gpu_suite) {
        return mark_fallen_and_continue(in_data);
    }

    PF_EffectWorld *input_world  = NULL;
    PF_EffectWorld *output_world = NULL;
    void           *src_buf      = NULL;
    void           *dst_buf      = NULL;

    err = extraP->cb->checkout_layer_pixels(in_data->effect_ref, PARAM_INPUT, &input_world);
    if (err || !input_world) {
        in_data->pica_basicP->ReleaseSuite(kPFGPUDeviceSuite, kPFGPUDeviceSuiteVersion1);
        return mark_fallen_and_continue(in_data);
    }
    err = extraP->cb->checkout_output(in_data->effect_ref, &output_world);
    if (err || !output_world) {
        extraP->cb->checkin_layer_pixels(in_data->effect_ref, PARAM_INPUT);
        in_data->pica_basicP->ReleaseSuite(kPFGPUDeviceSuite, kPFGPUDeviceSuiteVersion1);
        return mark_fallen_and_continue(in_data);
    }

    err = gpu_suite->GetGPUWorldData(in_data->effect_ref, input_world,  &src_buf);
    if (!err) err = gpu_suite->GetGPUWorldData(in_data->effect_ref, output_world, &dst_buf);
    if (err || !src_buf || !dst_buf) {
        extraP->cb->checkin_layer_pixels(in_data->effect_ref, PARAM_INPUT);
        in_data->pica_basicP->ReleaseSuite(kPFGPUDeviceSuite, kPFGPUDeviceSuiteVersion1);
        return mark_fallen_and_continue(in_data);
    }

    // BGRA128 GPU world: 16 bytes per pixel. The Rust kernel expects
    // pitches in pixels; AE delivers rowbytes in bytes.
    const uint32_t src_pitch_pixels = (uint32_t)(input_world->rowbytes  / 16);
    const uint32_t dst_pitch_pixels = (uint32_t)(output_world->rowbytes / 16);
    const uint32_t width            = (uint32_t)input_world->width;
    const uint32_t height           = (uint32_t)input_world->height;

    // From this point on we have valid src/dst MTLBuffers. Every error
    // path below MUST fill dst (passthrough fallback) before returning, or
    // AE shows the "smooth did not render anything" warning + dispatches
    // FrameTask 517 errors. Centralised cleanup-and-return at end.
    PF_Err final_err = PF_Err_NONE;

    // pre_render_data may be null in AE preview/cache edge cases (Phase
    // 2-A.1 follow-up note: "FrameTask threw 517 × 3" with same root). Do
    // NOT return PF_Err_INTERNAL_STRUCT_DAMAGED here — that is exactly
    // what AE wraps as the FrameTask error. Fall back to passthrough so
    // dst is filled, return NONE.
    SmartRenderInfo *info = (SmartRenderInfo*)extraP->input->pre_render_data;

    // §4.4 fault injection: simulate render-time failure / VRAM OOM.
    const bool inject_fail = info && (
        smooth_core_gpu_should_force_error(2) /* render */ ||
        smooth_core_gpu_should_force_error(3) /* oom    */);

    if (info == NULL || inject_fail) {
        // Fallback path: passthrough only so dst has the input pixels.
        gpu_passthrough_to_dst(
            metal_handle, src_buf, dst_buf,
            src_pitch_pixels, dst_pitch_pixels, width, height);
        if (inject_fail) {
            // Genuine GPU failure — mark fallen so the next PreRender
            // routes to CPU SmartRender for this instance.
            uint64_t uuid_lo, uuid_hi;
            if (read_sequence_uuid(in_data, &uuid_lo, &uuid_hi)) {
                smooth_core_gpu_mark_fallen(uuid_lo, uuid_hi);
            }
        }
        // info == NULL is an AE quirk, not a GPU failure — do NOT mark fallen.
    } else {
        const uint32_t white_opt = info->white_option ? 1u : 0u;
        // Match the CPU side's range_f32 derivation in smooth_core.h::
        // smoothing<> for the 32bpc branch: slider × max(=1.0) × 4 / 100.
        const float range_f32 = (float)((info->range * 4.0) / 100.0);
        // Match the CPU side's line_weight derivation
        // (info->line_weight / 2.0 + 0.5). Used by the per-output
        // mode_flg=15 outside kernel.
        const float line_weight = (float)(info->line_weight / 2.0 + 0.5);

        // Sub-stage C-2.5b.2-prep2c-step1 (FFI 0x0002_000f+, 2026-05-05):
        // Hybrid Path β. Two-pass GPU dispatch — pass 1 (smooth_detect)
        // writes 1 byte/pixel mode_flg metadata, pass 2 (smooth_per_pixel)
        // reads metadata + src and writes dst with cap-range cardinal
        // early-out for flat regions. The metadata buffer is AE-managed
        // (allocated via PF_GPUDeviceSuite::AllocateDeviceMemory per the
        // prep2b.2a foundation pattern that PASSed at real-device UAT).
        // Cap is read at run-time by Rust from env var
        // SMOOTH_GPU_MAX_LENGTH (default 32). See
        // docs/PHASE_2A_PREP2B_DESIGN_MEMO.md §10.
        //
        // CPU equivalence at the cap is intentionally broken in step1.
        // step2 will share the cap with the CPU path under the GPU
        // profile flag for network-render / mid-stream-fallback
        // continuity. Step1 builds are therefore NOT for shipping.
        uint64_t cb_uuid_lo = 0, cb_uuid_hi = 0;
        (void)read_sequence_uuid(in_data, &cb_uuid_lo, &cb_uuid_hi);

        // Allocate metadata buffer (1 byte per pixel) via the GPU suite.
        // Size = width * height bytes; allocations through this API are
        // visible to AE's GPU world synchroniser (proven safe by the
        // prep2b.2a foundation, commit fd2aa05, on the same MFR + 4400²
        // + 32bpc + 19-frame preview test).
        void *metadata_buf = NULL;
        const A_u_long device_index = extraP->input->device_index;
        const size_t metadata_bytes = (size_t)width * (size_t)height;
        PF_Err alloc_err = gpu_suite->AllocateDeviceMemory(
            in_data->effect_ref, device_index, metadata_bytes, &metadata_buf);

        if (alloc_err || !metadata_buf) {
            // OOM or other allocation failure — passthrough fill dst
            // and mark fallen so the next frame goes to CPU.
            gpu_passthrough_to_dst(
                metal_handle, src_buf, dst_buf,
                src_pitch_pixels, dst_pitch_pixels, width, height);
            if (cb_uuid_lo || cb_uuid_hi) {
                smooth_core_gpu_mark_fallen(cb_uuid_lo, cb_uuid_hi);
            }
        } else {
            int32_t rc = smooth_core_metal_dispatch_smooth_chain(
                metal_handle, src_buf, dst_buf, metadata_buf,
                src_pitch_pixels, dst_pitch_pixels,
                width, height, /* logical_width */ width,
                range_f32, white_opt, line_weight,
                cb_uuid_lo, cb_uuid_hi);

            if (rc != 0) {
                gpu_passthrough_to_dst(
                    metal_handle, src_buf, dst_buf,
                    src_pitch_pixels, dst_pitch_pixels, width, height);
                // The async completed handler will also mark_fallen on
                // GPU error after this point — that path covers the
                // silent-fail case where commit() returned but the GPU
                // failed during execution. Marking here too is idempotent.
                if (cb_uuid_lo || cb_uuid_hi) {
                    smooth_core_gpu_mark_fallen(cb_uuid_lo, cb_uuid_hi);
                }
            }

            // Free the metadata buffer in all paths. The kernels accessed
            // it via raw MTLBuffer pointers; cb.commit() has already
            // been called by Rust, but AE owns the device memory and
            // schedules its release after kernel completion (no manual
            // wait_until_completed needed on production path — env var
            // SMOOTH_GPU_INFLIGHT_LIMIT=1 path is diagnostic only).
            gpu_suite->FreeDeviceMemory(in_data->effect_ref, device_index, metadata_buf);
        }
    }

    extraP->cb->checkin_layer_pixels(in_data->effect_ref, PARAM_INPUT);
    in_data->pica_basicP->ReleaseSuite(kPFGPUDeviceSuite, kPFGPUDeviceSuiteVersion1);
    return final_err;
#else
    // Non-Apple builds: Sub-stage E will replace this with the CUDA path.
    (void)extraP;
    return mark_fallen_and_continue(in_data);
#endif
#endif
}

//---------------------------------------------------------------------------//
// ダイアログ作成
//---------------------------------------------------------------------------//
static PF_Err 
PopDialog (	
	PF_InData		*in_data,
	PF_OutData		*out_data,
	PF_ParamDef		*params[],
	PF_LayerDef		*output )
{
	PF_Err err = PF_Err_NONE;
 
	char str[256];
    memset( str, 0, 256 );

    sprintf(    out_data->return_msg, 
                 "%s, v%d.%d.%d\n%s\n",
                NAME, 
                MAJOR_VERSION, 
                MINOR_VERSION,
                BUILD_VERSION,
                str );

	return err;
}

