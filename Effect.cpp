
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

#include "AEConfig.h"
#include "AE_Effect.h"
#include "AE_EffectCB.h"
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
    PARAM_BUILD_INFO,   // 読み取り専用の Build 表示(偽成功判別用)
    PARAM_NUM,
};

// Phase 2-A.1: SmartRender が PreRender → Render の 2 段階になるため、
// 非 layer params の値を PreRender 時点で snapshot してから Render に渡す。
// pre_render_data は AE が delete_pre_render_data_func 経由で解放する。
struct SmartRenderInfo
{
    double range;          // PARAM_RANGE.fs_d.value
    double line_weight;    // PARAM_LINE_WEIGHT.fs_d.value
    bool   white_option;   // PARAM_WHITE_OPTION.bd.value
};

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
// (PF_Cmd_SMART_RENDER_GPU)は Phase 2-A.3 で追加。
static PF_Err SmartPreRender(PF_InData         *in_data,
                             PF_OutData        *out_data,
                             PF_PreRenderExtra *extraP);

static PF_Err SmartRender(PF_InData            *in_data,
                          PF_OutData           *out_data,
                          PF_SmartRenderExtra  *extraP);


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

	// input buffer を加工します
    out_data->out_flags  |= PF_OutFlag_I_WRITE_INPUT_BUFFER | PF_OutFlag_DEEP_COLOR_AWARE;
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
    //   Pipl.r::AE_Effect_Global_OutFlags_2 と**常に同期**(現値 0x08800410)。
    out_data->out_flags2 |= PF_OutFlag2_I_AM_THREADSAFE
                          | PF_OutFlag2_SUPPORTS_THREADED_RENDERING
                          | PF_OutFlag2_SUPPORTS_GET_FLATTENED_SEQUENCE_DATA
                          | PF_OutFlag2_SUPPORTS_SMART_RENDER;

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
	PF_Err	err;

    BEGIN_PROFILE();
    SMOOTH_BENCH_TIMER_BEGIN();

    err = PF_Err_NONE;

    // 以前は PF_COPY(input, output) をここで実行していたが、white_option 時に
    // 内部の白ピクセルが out に反映されないバグの原因となっていた。
    // smooth_core::process() が preProcess → in_ptr in-place 改変 → out_ptr へ
    // memcpy の順でコピーを担うため、AE SDK レベルのコピーは不要。

    // パラメータを core 形式へ変換(SmartRenderInfo の raw slider 値ベース)
    smooth_core::Params core_params;
    core_params.range        = (unsigned int)(info->range * (getMaxValue<PixelType>() * 4)) / 100;
    core_params.line_weight  = (float)(info->line_weight / 2.0 + 0.5);
    core_params.white_option = info->white_option;

    // AE SDK 非依存のコア処理を呼ぶ
    smooth_core::process<PixelType>(in_ptr, out_ptr,
                                    input->width, input->height, input->rowbytes,
                                    core_params);



    END_PROFILE();

    SMOOTH_BENCH_CAPTURE(
        GET_WIDTH(input),
        GET_HEIGHT(input),
        (int)(sizeof(PixelType) * 8 / 4),           // bpc: Pixel8 -> 8, Pixel16 -> 16 (div by 4 channels)
        input->rowbytes,
        in_ptr,
        out_ptr,
        (uint32_t)((unsigned int)(info->range * (getMaxValue<PixelType>() * 4)) / 100),
        (float)(info->line_weight / 2.0 + 0.5),
        info->white_option ? 1 : 0);

	return err;
}

// PARAM_RANGE / PARAM_LINE_WEIGHT / PARAM_WHITE_OPTION を SmartRenderInfo に
// snapshot するユーティリティ。Render(legacy)は params[] から、
// SmartRender は pre_render_data 経由で値を取る形に統一する。
static inline void params_to_smart_info(PF_ParamDef *params[], SmartRenderInfo *info)
{
    info->range        = params[PARAM_RANGE]->u.fs_d.value;
    info->line_weight  = params[PARAM_LINE_WEIGHT]->u.fs_d.value;
    info->white_option = params[PARAM_WHITE_OPTION]->u.bd.value ? true : false;
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

	PF_Pixel16	*in_ptr16, *out_ptr16;
	PF_GET_PIXEL_DATA16(output, NULL, &out_ptr16 );
	PF_GET_PIXEL_DATA16(input, NULL, &in_ptr16 );

	if( out_ptr16 != NULL && in_ptr16 != NULL )
	{
		// 16bpc or 32bpc
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

    // bpc 判定は legacy Render と同じパターン:PF_GET_PIXEL_DATA16 が NULL を
    // 返すかで 8/16bpc を区別。SmartRender 経路でも PF_LayerDef === PF_EffectWorld
    // typedef なので同 macro が使える。
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

    extraP->cb->checkin_layer_pixels(in_data->effect_ref, PARAM_INPUT);
    return err;
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

