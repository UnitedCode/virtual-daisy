This Apache-2.0 licensed ARM Cortex-M interpreter is derived from
https://github.com/jjkt/zmu/tree/72feb753fea97babc00cc88f2c75e814efc1bfd2/zmu_cortex_m
by Jarmo Torvinen. The upstream license is preserved in LICENSE.

Local changes support STM32H750 browser and native hosts. They include FPv5-D16
and DSP instructions, timed bus hooks, separate instruction fetch, restartable
accesses, architectural fault delivery, NVIC readbacks, SysTick clock selection
and exception frame handling. Scalar/paired VMOV transfers and signed bit-field
extraction have independent register/result checks. Bus transactions remain
opt-in for existing hosts.

Changes are checked against the upstream instruction suite, executed ARM guests,
the pinned Renode reference backend and ST's PM0253 programming manual. See
../docs/rust-parity.md for the compared cases and remaining timing limits.
