
#ifndef __EFFECT_H
#define __EFFECT_H

#include <entry.h>



// NOTE: signature must match the definition in Effect.cpp exactly — the
// extern "C" linkage is what makes AE able to resolve the symbol by the name
// "EntryPointFunc" declared in Pipl.r. If this header and the .cpp disagree,
// C++ treats them as different overloads and the exported symbol becomes
// name-mangled (e.g. `__Z14EntryPointFunciP9PF_InData...Pv`), so AE reports
// "Couldn't find main entry point for smooth.plugin" and the effect becomes
// "Missing Effect". The 6th arg `void *extra` was added in 2026-04-22 to
// handle PF_Cmd_USER_CHANGED_PARAM for the Build button.
extern "C"
DllExport
PF_Err EntryPointFunc(
    PF_Cmd          cmd,
    PF_InData       *in_data,
    PF_OutData      *out_data,
    PF_ParamDef     *params[],
    PF_LayerDef     *output,
    void            *extra );


#endif
