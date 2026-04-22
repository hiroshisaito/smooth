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
			0x2000800
		},
		AE_Effect_Global_OutFlags_2 {
			0x08800010 /* I_AM_THREADSAFE (bit 4 = 0x10) | SUPPORTS_GET_FLATTENED_SEQUENCE_DATA (bit 23 = 0x00800000) | SUPPORTS_THREADED_RENDERING (bit 27 = 0x08000000); must match Effect.cpp GlobalSetup out_flags2 */
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

