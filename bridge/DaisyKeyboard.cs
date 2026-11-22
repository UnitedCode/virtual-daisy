// Mandatory Renode dispatch only; carrier wiring, FIFO pacing and measurements
// live in Rust. This executes the supplied ELF without modifying guest RAM/code.
using System;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using Antmicro.Renode.Core;
using Antmicro.Renode.Peripherals;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.CPU;
using Antmicro.Renode.Time;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    [AllowedTranslations(AllowedTranslation.ByteToDoubleWord | AllowedTranslation.WordToDoubleWord)]
    public sealed class DaisyKeyboardUart : IDoubleWordPeripheral,IKnownSize,IDisposable
    {
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate IntPtr New(uint instant);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void Free(IntPtr state);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate ulong Uart(IntPtr state,uint offset,uint value,uint write,ulong now,ulong hz);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint Input(IntPtr state,uint mux,uint channel,uint uv);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint Sample(IntPtr state,uint adc,uint pb,uint pc,uint pg,ulong now);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate uint Save(IntPtr state,[MarshalAs(UnmanagedType.LPUTF8Str)]string path,ulong now);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate ulong Domain(IntPtr state,uint domain);
        [UnmanagedFunctionPointer(CallingConvention.Cdecl)] private delegate void Clear(IntPtr state,ulong now);
        public DaisyKeyboardUart(IMachine machine,string libraryPath,DaisyRcc rcc,uint frequency=200000000)
        {
            this.machine=machine;this.rcc=rcc;
            var library=NativeLibrary.Load(libraryPath);
            State=Marshal.GetDelegateForFunctionPointer<New>(NativeLibrary.GetExport(library,"daisy_keyboard_new"))(0);
            uart=Marshal.GetDelegateForFunctionPointer<Uart>(NativeLibrary.GetExport(library,"daisy_keyboard_uart"));
            input=Marshal.GetDelegateForFunctionPointer<Input>(NativeLibrary.GetExport(library,"daisy_keyboard_input"));
            sample=Marshal.GetDelegateForFunctionPointer<Sample>(NativeLibrary.GetExport(library,"daisy_keyboard_sample"));
            save=Marshal.GetDelegateForFunctionPointer<Save>(NativeLibrary.GetExport(library,"daisy_keyboard_save"));
            domain=Marshal.GetDelegateForFunctionPointer<Domain>(NativeLibrary.GetExport(library,"daisy_clock_domain"));
            free=Marshal.GetDelegateForFunctionPointer<Free>(NativeLibrary.GetExport(library,"daisy_keyboard_free"));
            clear=Marshal.GetDelegateForFunctionPointer<Clear>(NativeLibrary.GetExport(library,"daisy_keyboard_clear"));
            reset=Marshal.GetDelegateForFunctionPointer<Clear>(NativeLibrary.GetExport(library,"daisy_keyboard_reset_uart"));
            create=Marshal.GetDelegateForFunctionPointer<New>(NativeLibrary.GetExport(library,"daisy_keyboard_new"));
            IRQ=new GPIO();
        }
        public void Configure(bool instant) {free(State);State=create(instant?1U:0U);}
        public void SetInput(uint mux,uint channel,uint uv) {if(input(State,mux,channel,uv)!=0) throw new InvalidDataException("Invalid mux input");}
        public uint SampleInput(int channel) {
            uint result=sample(State,(uint)channel,machine.SystemBus.ReadDoubleWord(0x58020414),machine.SystemBus.ReadDoubleWord(0x58020814),machine.SystemBus.ReadDoubleWord(0x58021814),Now);
            if(result==uint.MaxValue) throw new InvalidOperationException($"No enabled carrier mux for ADC channel {channel}");
            return result;
        }
        public void SaveResult(string path) {if(save(State,path,Now)!=0)throw new IOException("Cannot save keyboard evidence");}
        public void ClearObservations()=>clear(State,Now);
        private ulong Access(long offset,uint value,uint write) {
            if(machine.SystemBus.TryGetCurrentCPU(out var cpu))cpu.SyncTime();
            if((rcc.ReadDoubleWord(0x54)&0x38)!=0)throw new InvalidOperationException("Keyboard UART currently requires PCLK2 kernel selection");
            ulong result=uart(State,(uint)offset,value,write,Now,domain(rcc.NativeState,3));
            if(result==ulong.MaxValue)throw new InvalidOperationException($"Unsupported keyboard UART access 0x{offset:X}, value=0x{value:X}");
            return result;
        }
        public uint ReadDoubleWord(long offset)=>(uint)Access(offset,0,0);
        public void WriteDoubleWord(long offset,uint value)=>Access(offset,value,1);
        public void Reset() {if(State!=IntPtr.Zero)reset(State,Now);IRQ.Unset();}
        public void Dispose() {if(State!=IntPtr.Zero){free(State);State=IntPtr.Zero;}}
        public long Size=>0x400;
        public GPIO IRQ {get;}
        public IntPtr State {get;private set;}
        private ulong Now=>machine.ElapsedVirtualTime.TimeElapsed.TotalNanoseconds;
        private readonly IMachine machine;private readonly DaisyRcc rcc;
        private readonly Uart uart;private readonly Input input;private readonly Sample sample;private readonly Save save;private readonly Domain domain;private readonly Free free;private readonly New create;
        private readonly Clear clear,reset;
    }
}

public static class DaisyKeyboardHost
{
    public static void Run(Emulation emulation,Machine machine,string prefix,string scenario,bool instant)
    {
        var uart=(Antmicro.Renode.Peripherals.Miscellaneous.DaisyKeyboardUart)machine["sysbus.usart1"];
        uart.Configure(instant);
        ((Antmicro.Renode.Peripherals.Miscellaneous.DaisyAdc)machine["sysbus.adcM1S2"]).AttachInputs(uart.SampleInput);
        var lines=File.ReadAllLines(scenario).Where(x=>x.Length!=0&&!x.StartsWith("#")).Select(x=>x.Split(',')).ToArray();
        ulong previous=0;
        foreach(var fields in lines) {
            if(fields.Length<2 || !ulong.TryParse(fields[0],out var time) || time<previous)
                throw new InvalidDataException("Invalid or nonmonotonic keyboard scenario");
            previous=time;
            bool valid=(fields[1]=="reset" && fields.Length==2)
                || (fields[1]=="input" && fields.Length==5)
                || (fields[1]=="save" && fields.Length==3 && fields[2].Length>0 && fields[2].All(c=>char.IsLetterOrDigit(c)||c=='-'||c=='_'));
            if(!valid)throw new InvalidDataException("Invalid keyboard scenario action");
        }
        using var log=new StreamWriter(prefix+".rtt");
        var bus=(SystemBus)machine["sysbus"];
        System.Collections.Generic.IEnumerable<ulong> addresses;
        ulong rtt=bus.TryGetAllSymbolAddresses("_SEGGER_RTT",out addresses)?addresses.Single():0;
        foreach(var fields in lines) {
            ulong target=ulong.Parse(fields[0]);
            ulong now=machine.ElapsedVirtualTime.TimeElapsed.TotalNanoseconds;
            if(target<now)throw new InvalidDataException("Keyboard scenario time moved backwards");
            while(now<target) {
                ulong chunk=Math.Min(target-now,100000000UL);
                emulation.RunFor(TimeInterval.FromNanoseconds(chunk));
                now=machine.ElapsedVirtualTime.TimeElapsed.TotalNanoseconds;
                if(rtt!=0) {
                    uint buffer=bus.ReadDoubleWord(rtt+28),capacity=bus.ReadDoubleWord(rtt+32),write=bus.ReadDoubleWord(rtt+36),read=bus.ReadDoubleWord(rtt+40);
                    if(capacity>0&&capacity<=65536&&read<capacity&&write<capacity) {
                        while(read!=write){log.Write((char)bus.ReadByte(buffer+read));read=(read+1)%capacity;}
                        bus.WriteDoubleWord(rtt+40,read);log.Flush();
                    }
                }
                if(chunk==100000000UL)Console.WriteLine($"Keyboard progress: {now/1000000} ms, PC: {((CortexM)machine["sysbus.cpu"]).PC}");
            }
            if(fields[1]=="input")uart.SetInput(uint.Parse(fields[2]),uint.Parse(fields[3]),uint.Parse(fields[4]));
            else if(fields[1]=="save")uart.SaveResult(prefix+"."+fields[2]);
            else if(fields[1]=="reset")uart.ClearObservations();
            else throw new InvalidDataException("Unknown keyboard scenario action");
        }
        uart.SaveResult(prefix);
        Console.WriteLine($"Keyboard complete: PC: {((CortexM)machine["sysbus.cpu"]).PC}");
    }
}
