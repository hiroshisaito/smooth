#include "AEConfig.h"
#include "AE_EffectVers.h"

#ifndef AE_OS_WIN
	#include "AE_General.r"
#endif

resource 'PiPL' (16000) {
	{	/* array properties: 12 elements */
		/* [1] */
		Kind {
			AEEffect
		},
		/* [2] */
		Name {
			"smooth"
		},
		/* [3] */
		Category {
			"LoiLo"
		},
		
#ifdef AE_OS_WIN
	#ifdef AE_PROC_INTELx64
		CodeWin64X86 {"EntryPointFunc"},
	#else
		CodeWin32X86 {"EntryPointFunc"},
	#endif	
#else
	#ifdef AE_OS_MAC
			CodeMachOPowerPC {"EntryPointFunc"},
			CodeMacIntel32 {"EntryPointFunc"},
			CodeMacIntel64 {"EntryPointFunc"},
			CodeMacARM64   {"EntryPointFunc"},
	#endif
#endif

		/* [6] */
		AE_PiPL_Version {
			2,
			0
		},
		/* [7] */
		AE_Effect_Spec_Version {
			PF_PLUG_IN_VERSION,
			PF_PLUG_IN_SUBVERS
		},
		/* [8] */
		AE_Effect_Version {
			1049088 /* PF_VERSION(2,0,0,1,0) = 0x100200; must match Effect.cpp GlobalSetup my_version */
		},
		/* [9] */
		AE_Effect_Info_Flags {
			0
		},
		/* [10] */
		AE_Effect_Global_OutFlags {
			/* Phase 2-A.1: PF_OutFlag_I_WRITE_INPUT_BUFFER (bit 11 = 0x800)
			   removed because AE 2025 forbids it together with
			   PF_OutFlag2_SUPPORTS_SMART_RENDER (verifier failure +
			   render-thread SIGSEGV observed on 2026-05-03).
			   Remaining: PF_OutFlag_DEEP_COLOR_AWARE (bit 25) only.
			   smoothing<>() now allocates its own scratch buffer for the
			   in-place preProcess step. Must match Effect.cpp GlobalSetup
			   out_flags. */
			0x2000000
		},
		AE_Effect_Global_OutFlags_2 {
			/* Phase 2-A.3 Sub-stage C-2: SUPPORTS_GPU_RENDER_F32 (bit 25 = 0x02000000)
			   added so AE will issue PF_Cmd_GPU_DEVICE_SETUP / SMART_RENDER_GPU
			   to this effect on 32bpc compositions. Per AE_Effect.h L1007, this
			   flag must ALSO be set in (a) Effect.cpp GlobalSetup out_flags2 and
			   (b) Effect.cpp GPU_DEVICE_SETUP out_data->out_flags2 — three places
			   in total. Missing any one causes AE to silently route everything
			   to CPU SmartRender (no error, just no GPU calls).
			   I_AM_THREADSAFE (bit 4 = 0x10) | SUPPORTS_SMART_RENDER (bit 10 = 0x400)
			   | FLOAT_COLOR_AWARE (bit 12 = 0x1000)
			   | SUPPORTS_GET_FLATTENED_SEQUENCE_DATA (bit 23 = 0x00800000)
			   | SUPPORTS_GPU_RENDER_F32 (bit 25 = 0x02000000)
			   | SUPPORTS_THREADED_RENDERING (bit 27 = 0x08000000)
			   = 0x0A801410. Must match Effect.cpp GlobalSetup out_flags2. */
			0x0A801410
		},
		/* [11] */
		AE_Effect_Match_Name {
			"KOJI_SMOOTH"
		},
		/* [12] */
		AE_Reserved_Info {
			8
		}
	}
};

