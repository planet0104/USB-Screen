using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using LibreHardwareMonitor.Hardware;

namespace LhmNativeAotWrapper;

internal sealed class UpdateVisitor : IVisitor
{
    public void VisitComputer(IComputer computer)
    {
        computer.Traverse(this);
    }

    public void VisitHardware(IHardware hardware)
    {
        hardware.Update();
        foreach (IHardware subHardware in hardware.SubHardware)
        {
            subHardware.Accept(this);
        }
    }

    public void VisitSensor(ISensor sensor)
    {
    }

    public void VisitParameter(IParameter parameter)
    {
    }
}

[JsonSourceGenerationOptions(PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase)]
[JsonSerializable(typeof(HardwareData))]
[JsonSerializable(typeof(AllSensorData))]
internal partial class HardwareJsonContext : JsonSerializerContext
{
}

public sealed class HardwareInfo
{
    public List<float> fans { get; set; } = [];
    public List<float> temperatures { get; set; } = [];
    public List<float> loads { get; set; } = [];
    public List<float> clocks { get; set; } = [];
    public float package_power { get; set; }
    public float cores_power { get; set; }
    public float total_load { get; set; }
    public float total_temperature { get; set; }
    public float memory_load { get; set; }
    public float memory_total { get; set; }
}

public sealed class HardwareData
{
    public List<HardwareInfo> cpu_infos { get; set; } = [];
    public List<HardwareInfo> gpu_infos { get; set; } = [];
}

public sealed class SensorInfo
{
    public string hardware_type { get; set; } = string.Empty;
    public string hardware_name { get; set; } = string.Empty;
    public string hardware_identifier { get; set; } = string.Empty;
    public string sensor_type { get; set; } = string.Empty;
    public string sensor_name { get; set; } = string.Empty;
    public string sensor_identifier { get; set; } = string.Empty;
    public float value { get; set; }
}

public sealed class AllSensorData
{
    public List<SensorInfo> sensors { get; set; } = [];
}

[StructLayout(LayoutKind.Sequential)]
public unsafe struct NativeBuffer
{
    public byte* ptr;
    public nuint len;
}

public static class HardwareWrapper
{
    private static readonly object SyncRoot = new();
    private static readonly UpdateVisitor Visitor = new();
    private static Computer? _computer;
    private static string _lastError = string.Empty;
    private static bool _enhancedSensorsEnabled;

    [UnmanagedCallersOnly(EntryPoint = "lhm_init")]
    public static int Init()
    {
        try
        {
            lock (SyncRoot)
            {
                if (_computer is not null)
                {
                    _lastError = string.Empty;
                    return 0;
                }

                TryInitializeComputer();
            }

            ClearLastError();
            return 0;
        }
        catch (Exception ex)
        {
            return SetLastError(ex);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "lhm_update")]
    public static int Update()
    {
        try
        {
            lock (SyncRoot)
            {
                if (_computer is null)
                {
                    throw new InvalidOperationException("hardware monitor not initialized");
                }

                _computer.Accept(Visitor);
            }

            ClearLastError();
            return 0;
        }
        catch (Exception ex)
        {
            return SetLastError(ex);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "lhm_get_json")]
    public static NativeBuffer GetJson()
    {
        try
        {
            HardwareData data;
            lock (SyncRoot)
            {
                if (_computer is null)
                {
                    throw new InvalidOperationException("hardware monitor not initialized");
                }

                data = CollectHardwareData(_computer);
            }

            string json = JsonSerializer.Serialize(data, HardwareJsonContext.Default.HardwareData);
            ClearLastError();
            return ToNativeBuffer(json);
        }
        catch (Exception ex)
        {
            SetLastError(ex);
            return default;
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "lhm_get_all_sensors_json")]
    public static NativeBuffer GetAllSensorsJson()
    {
        try
        {
            AllSensorData data;
            lock (SyncRoot)
            {
                if (_computer is null)
                {
                    throw new InvalidOperationException("hardware monitor not initialized");
                }

                data = CollectAllSensors(_computer);
            }

            string json = JsonSerializer.Serialize(
                data,
                HardwareJsonContext.Default.AllSensorData);
            ClearLastError();
            return ToNativeBuffer(json);
        }
        catch (Exception ex)
        {
            SetLastError(ex);
            return default;
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "lhm_get_last_error")]
    public static NativeBuffer GetLastError()
    {
        lock (SyncRoot)
        {
            return ToNativeBuffer(_lastError);
        }
    }

    [UnmanagedCallersOnly(EntryPoint = "lhm_free_buffer")]
    public static unsafe void FreeBuffer(byte* ptr, nuint _len)
    {
        if (ptr == null)
        {
            return;
        }

        Marshal.FreeHGlobal((nint)ptr);
    }

    [UnmanagedCallersOnly(EntryPoint = "lhm_close")]
    public static void Close()
    {
        lock (SyncRoot)
        {
            if (_computer is not null)
            {
                _computer.Close();
                _computer = null;
            }

            _lastError = string.Empty;
            _enhancedSensorsEnabled = false;
        }
    }

    private static void TryInitializeComputer()
    {
        try
        {
            _computer = CreateComputer(enableEnhancedSensors: true);
            _computer.Open();
            _enhancedSensorsEnabled = true;
        }
        catch
        {
            try
            {
                _computer?.Close();
            }
            catch
            {
            }

            _computer = CreateComputer(enableEnhancedSensors: false);
            _computer.Open();
            _enhancedSensorsEnabled = false;
        }
    }

    private static Computer CreateComputer(bool enableEnhancedSensors)
    {
        return new Computer
        {
            IsCpuEnabled = true,
            IsGpuEnabled = true,
            IsControllerEnabled = enableEnhancedSensors,
            IsMemoryEnabled = true,
            IsMotherboardEnabled = enableEnhancedSensors,
            // 当前 Rust 侧只消费 CPU/GPU 数据 关闭存储链路可避免额外的设备监听副作用
            IsStorageEnabled = false
        };
    }

    private static HardwareData CollectHardwareData(Computer computer)
    {
        var data = new HardwareData();

        foreach (IHardware hardware in computer.Hardware)
        {
            CollectHardwareRecursive(hardware, data);
        }

        return data;
    }

    private static AllSensorData CollectAllSensors(Computer computer)
    {
        var data = new AllSensorData();
        foreach (IHardware hardware in computer.Hardware)
        {
            CollectAllSensorsRecursive(hardware, data);
        }

        return data;
    }

    private static void CollectHardwareRecursive(IHardware hardware, HardwareData data)
    {
        string hardwareType = hardware.HardwareType.ToString();
        HardwareInfo? hardwareInfo = null;

        if (string.Equals(hardwareType, "Cpu", StringComparison.Ordinal))
        {
            hardwareInfo = new HardwareInfo();
            data.cpu_infos.Add(hardwareInfo);
        }
        else if (hardwareType.StartsWith("Gpu", StringComparison.Ordinal))
        {
            hardwareInfo = new HardwareInfo();
            data.gpu_infos.Add(hardwareInfo);
        }

        if (hardwareInfo is not null)
        {
            FillHardwareInfo(hardware, hardwareInfo, hardwareType.StartsWith("Gpu", StringComparison.Ordinal));
        }
        else
        {
            CollectSupplementalHardware(hardware, data);
        }

        foreach (IHardware subHardware in hardware.SubHardware)
        {
            CollectHardwareRecursive(subHardware, data);
        }
    }

    private static void CollectAllSensorsRecursive(IHardware hardware, AllSensorData data)
    {
        foreach (ISensor sensor in hardware.Sensors)
        {
            if (sensor.Value is not float value)
            {
                continue;
            }

            data.sensors.Add(new SensorInfo
            {
                hardware_type = hardware.HardwareType.ToString(),
                hardware_name = hardware.Name ?? string.Empty,
                hardware_identifier = hardware.Identifier.ToString(),
                sensor_type = sensor.SensorType.ToString(),
                sensor_name = sensor.Name ?? string.Empty,
                sensor_identifier = sensor.Identifier.ToString(),
                value = value
            });
        }

        foreach (IHardware subHardware in hardware.SubHardware)
        {
            CollectAllSensorsRecursive(subHardware, data);
        }
    }

    private static void CollectSupplementalHardware(IHardware hardware, HardwareData data)
    {
        foreach (ISensor sensor in hardware.Sensors)
        {
            if (sensor.Value is not float value)
            {
                continue;
            }

            SensorTarget target = ClassifySensorTarget(hardware, sensor);
            if (target == SensorTarget.None)
            {
                continue;
            }

            if (target == SensorTarget.Cpu)
            {
                HardwareInfo cpuInfo = GetOrCreatePrimaryCpuInfo(data);
                ApplySensorToHardwareInfo(cpuInfo, sensor, value, false);
            }
            else if (target == SensorTarget.Gpu)
            {
                HardwareInfo gpuInfo = GetOrCreatePrimaryGpuInfo(data);
                ApplySensorToHardwareInfo(gpuInfo, sensor, value, true);
            }
        }
    }

    private static void FillHardwareInfo(IHardware hardware, HardwareInfo hardwareInfo, bool isGpu)
    {
        foreach (ISensor sensor in hardware.Sensors)
        {
            if (sensor.Value is not float value)
            {
                continue;
            }

            ApplySensorToHardwareInfo(hardwareInfo, sensor, value, isGpu);
        }
    }

    private static void ApplySensorToHardwareInfo(
        HardwareInfo hardwareInfo,
        ISensor sensor,
        float value,
        bool isGpu)
    {
        string sensorType = sensor.SensorType.ToString();
        string sensorName = sensor.Name ?? string.Empty;

        if (string.Equals(sensorType, "Temperature", StringComparison.Ordinal))
        {
            if (IsTotalTemperatureSensor(sensorName, isGpu))
            {
                hardwareInfo.total_temperature = value;
            }

            hardwareInfo.temperatures.Add(value);
            return;
        }

        if (string.Equals(sensorType, "Fan", StringComparison.Ordinal))
        {
            hardwareInfo.fans.Add(value);
            return;
        }

        if (string.Equals(sensorType, "Clock", StringComparison.Ordinal))
        {
            if (Contains(sensorName, "Bus") || Contains(sensorName, "Memory") || Contains(sensorName, "Shader"))
            {
                return;
            }

            hardwareInfo.clocks.Add(value);
            return;
        }

        if (string.Equals(sensorType, "Load", StringComparison.Ordinal))
        {
            if (Contains(sensorName, "Memory"))
            {
                hardwareInfo.memory_load = value;
            }
            else if (Contains(sensorName, "Total") || (isGpu && Contains(sensorName, "GPU Core")))
            {
                hardwareInfo.total_load = value;
            }
            else
            {
                hardwareInfo.loads.Add(value);
            }

            return;
        }

        if (string.Equals(sensorType, "Power", StringComparison.Ordinal))
        {
            if (Contains(sensorName, "Package"))
            {
                hardwareInfo.package_power = value;
            }
            else if (Contains(sensorName, "Cores") || Contains(sensorName, "Core"))
            {
                hardwareInfo.cores_power = value;
            }
            else if (hardwareInfo.package_power == 0.0f)
            {
                hardwareInfo.package_power = value;
            }

            return;
        }

        if ((sensorType.Contains("Data", StringComparison.Ordinal) || sensorType.Contains("SmallData", StringComparison.Ordinal))
            && Contains(sensorName, "GPU Memory Total"))
        {
            hardwareInfo.memory_total = value;
        }
    }

    private static bool IsTotalTemperatureSensor(string sensorName, bool isGpu)
    {
        if (Contains(sensorName, "Package")
            || Contains(sensorName, "Core Average")
            || Contains(sensorName, "Core Max")
            || Contains(sensorName, "CPU Package")
            || Contains(sensorName, "Tctl")
            || Contains(sensorName, "Tdie"))
        {
            return true;
        }

        if (isGpu
            && (Contains(sensorName, "GPU Core")
                || Contains(sensorName, "Hot Spot")
                || Contains(sensorName, "Junction")))
        {
            return true;
        }

        return false;
    }

    private static HardwareInfo GetOrCreatePrimaryCpuInfo(HardwareData data)
    {
        if (data.cpu_infos.Count == 0)
        {
            data.cpu_infos.Add(new HardwareInfo());
        }

        return data.cpu_infos[0];
    }

    private static HardwareInfo GetOrCreatePrimaryGpuInfo(HardwareData data)
    {
        if (data.gpu_infos.Count == 0)
        {
            data.gpu_infos.Add(new HardwareInfo());
        }

        return data.gpu_infos[0];
    }

    private static SensorTarget ClassifySensorTarget(IHardware hardware, ISensor sensor)
    {
        string hardwareText = string.Join(
            " ",
            hardware.HardwareType,
            hardware.Name ?? string.Empty,
            hardware.Identifier.ToString(),
            sensor.Name ?? string.Empty,
            sensor.Identifier.ToString());

        if (ContainsAny(hardwareText, "gpu", "nvidia", "geforce", "rtx", "gtx", "radeon", "rx"))
        {
            return SensorTarget.Gpu;
        }

        if (ContainsAny(
            hardwareText,
            "cpu",
            "processor",
            "package",
            "intel",
            "amd",
            "ryzen",
            "core",
            "peci",
            "tctl",
            "tdie"))
        {
            return SensorTarget.Cpu;
        }

        return SensorTarget.None;
    }

    private static bool Contains(string source, string value)
    {
        return source.Contains(value, StringComparison.OrdinalIgnoreCase);
    }

    private static bool ContainsAny(string source, params string[] values)
    {
        foreach (string value in values)
        {
            if (Contains(source, value))
            {
                return true;
            }
        }

        return false;
    }

    private enum SensorTarget
    {
        None = 0,
        Cpu = 1,
        Gpu = 2
    }

    private static int SetLastError(Exception ex)
    {
        lock (SyncRoot)
        {
            _lastError = ex.ToString();
        }

        return 1;
    }

    private static void ClearLastError()
    {
        lock (SyncRoot)
        {
            _lastError = string.Empty;
        }
    }

    private static unsafe NativeBuffer ToNativeBuffer(string? value)
    {
        if (string.IsNullOrEmpty(value))
        {
            return default;
        }

        byte[] bytes = Encoding.UTF8.GetBytes(value);
        nint memory = Marshal.AllocHGlobal(bytes.Length);
        Marshal.Copy(bytes, 0, memory, bytes.Length);

        return new NativeBuffer
        {
            ptr = (byte*)memory,
            len = (nuint)bytes.Length
        };
    }
}
