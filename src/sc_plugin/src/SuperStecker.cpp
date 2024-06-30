#include "SC_PlugIn.hpp"
#include "SuperStecker.hpp"

static InterfaceTable* ft;

namespace SuperStecker {

SuperStecker::SuperStecker() {
    mCalcFunc = make_calc_function<SuperStecker, &SuperStecker::next_k>();
    next_k(1);
}

void SuperStecker::next_k(int nSamples) {
    float msg = recv_message();
    out0(0) = msg;
}

} // namespace SuperStecker

PluginLoad(SuperSteckerUGens) {
    // Plugin magic
    ft = inTable;
    registerUnit<SuperStecker::SuperStecker>(ft, "SuperStecker", false);
}
