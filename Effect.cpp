
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

//---------------------------------------------------------------------------//
// 定義
enum 
{
    PARAM_INPUT = 0,
    PARAM_WHITE_OPTION,
    PARAM_RANGE,
    PARAM_LINE_WEIGHT,
    PARAM_NUM,
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
                            PF_LayerDef     *output )
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


            case PF_Cmd_RENDER:             // レンダリング
                err = Render(   in_data, 
                                out_data, 
                                params, 
                                output);
                break;

			case PF_Cmd_DO_DIALOG:
				err = PopDialog(in_data,
								out_data,
								params,
								output);
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

    sprintf(    out_data->return_msg, 
                 "%s, v%d.%d.%d %s\n%s\n",
                NAME, 
                MAJOR_VERSION, 
                MINOR_VERSION,
                BUILD_VERSION,
                stage_str,
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
    // 使いづらいから使わないので0固定
    out_data->my_version    = PF_VERSION(2,0,0,0,0);    

	// input buffer を加工します
    out_data->out_flags  |= PF_OutFlag_I_WRITE_INPUT_BUFFER | PF_OutFlag_DEEP_COLOR_AWARE;
    out_data->out_flags2 |= PF_OutFlag2_I_AM_THREADSAFE;

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
static PF_Err smoothing(PF_InData   *in_data,
						PF_OutData  *out_data,
                        PF_ParamDef *params[],
						PF_LayerDef *input,
						PF_LayerDef *output,
						PixelType	*in_ptr,
						PixelType	*out_ptr)
{
	PF_Err	err;

    BEGIN_PROFILE();
    SMOOTH_BENCH_TIMER_BEGIN();

    // AE 側で in → out に丸ごとコピー(その後 out を in-place 編集)
    err = PF_COPY(input, output, NULL, NULL);

    // パラメータを core 形式へ変換
    smooth_core::Params core_params;
    core_params.range        = (unsigned int)(params[PARAM_RANGE]->u.fs_d.value * (getMaxValue<PixelType>() * 4)) / 100;
    core_params.line_weight  = (float)(params[PARAM_LINE_WEIGHT]->u.fs_d.value / 2.0 + 0.5);
    core_params.white_option = params[PARAM_WHITE_OPTION]->u.bd.value ? true : false;

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
        (uint32_t)((unsigned int)(params[PARAM_RANGE]->u.fs_d.value * (getMaxValue<PixelType>() * 4)) / 100),
        (float)(params[PARAM_LINE_WEIGHT]->u.fs_d.value / 2.0 + 0.5),
        params[PARAM_WHITE_OPTION]->u.bd.value ? 1 : 0);

	return err;
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

	PF_Pixel16	*in_ptr16, *out_ptr16;
	PF_GET_PIXEL_DATA16(output, NULL, &out_ptr16 );
	PF_GET_PIXEL_DATA16(input, NULL, &in_ptr16 );

	if( out_ptr16 != NULL && in_ptr16 != NULL )
	{
		// 16bpc or 32bpc
		err = smoothing<PF_Pixel16, KP_PIXEL64>(in_data, out_data, params,
												input, output, in_ptr16, out_ptr16 );
	}
	else
	{
		// 8bpc
		PF_Pixel8	*in_ptr8, *out_ptr8;
		PF_GET_PIXEL_DATA8(output, NULL, &out_ptr8 );
		PF_GET_PIXEL_DATA8(input, NULL, &in_ptr8 );
		
		err = smoothing<PF_Pixel8, KP_PIXEL32>(in_data, out_data, params,
												input, output, in_ptr8, out_ptr8 );
	}

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

