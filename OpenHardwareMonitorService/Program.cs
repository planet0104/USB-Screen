using Newtonsoft.Json;
using OpenHardwareMonitor.Hardware;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Net.Http;
using System.Reflection;
using System.Text;
using System.Threading.Tasks;
using System.Threading;
using System.Windows.Forms;

namespace OpenHardwareMonitorService
{
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

        public void VisitSensor(ISensor sensor) { }

        public void VisitParameter(IParameter parameter) { }
    }

    internal sealed class HardwareInfo
    {
        public List<float> fans = new List<float>();
        public List<float> temperatures = new List<float>();
        public List<float> loads = new List<float>();
        public List<float> clocks = new List<float>();
        public List<float> memory_loads = new List<float>();
        public float package_power = 0;
        public float memory_total = 0;
        public float total_load = 0;
        public float total_temperature = 0;
    }

    internal sealed class HardwarePayload
    {
        public List<HardwareInfo> cpu_infos = new List<HardwareInfo>();
        public List<HardwareInfo> gpu_infos = new List<HardwareInfo>();
    }

    internal enum MonitorMode
    {
        Extended,
        Compatibility
    }

    internal static class Program
    {
        private static readonly HttpClient HttpClient = new HttpClient();
        private static readonly UpdateVisitor UpdateVisitor = new UpdateVisitor();
        private static string _baseUrl = "http://localhost/";
        private static string _logPath = string.Empty;

        static Program()
        {
            // 继续沿用旧服务的内嵌 DLL 加载方式 这样输出目录只需要 exe 和 config
            AppDomain.CurrentDomain.AssemblyResolve += ResolveEmbeddedAssembly;
            HttpClient.Timeout = TimeSpan.FromSeconds(3);
        }

        private static Assembly ResolveEmbeddedAssembly(object sender, ResolveEventArgs e)
        {
            string resourceName = "OpenHardwareMonitorService.libs." + new AssemblyName(e.Name).Name + ".dll";
            using (var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream(resourceName))
            {
                if (stream == null)
                {
                    return null;
                }

                byte[] data = new byte[stream.Length];
                stream.Read(data, 0, data.Length);
                return Assembly.Load(data);
            }
        }

        [STAThread]
        private static void Main(string[] args)
        {
            InitializeLogging();
            AppDomain.CurrentDomain.UnhandledException += OnUnhandledException;
            TaskScheduler.UnobservedTaskException += OnUnobservedTaskException;

            MonitorMode mode = ParseMonitorMode(args);
            if (args.Length > 0 && int.TryParse(args[0], out int port))
            {
                _baseUrl = "http://localhost:" + port + "/";
            }

            LogInfo("启动 OpenHardwareMonitorService mode=" + mode + " baseUrl=" + _baseUrl);

            try
            {
                using (Mutex mutex = new Mutex(true, Application.ProductName, out bool createdNew))
                {
                    if (!createdNew)
                    {
                        LogInfo("OpenHardwareMonitorService 已运行");
                        return;
                    }

                    LogInfo("准备创建 Computer 实例 mode=" + mode);
                    Computer computer = CreateComputer(mode);
                    LogInfo("Computer 实例创建完成 mode=" + mode);
                    bool firstPayloadLogged = false;
                    bool firstUploadLogged = false;
                    try
                    {
                        LogInfo("准备执行 computer.Open mode=" + mode);
                        computer.Open();
                        LogInfo("computer.Open 完成 mode=" + mode);
                        while (true)
                        {
                            try
                            {
                                if (!firstPayloadLogged)
                                {
                                    LogInfo("准备首次采集硬件数据 mode=" + mode);
                                }
                                HardwarePayload payload = CollectPayload(computer);
                                if (!firstPayloadLogged)
                                {
                                    LogInfo(
                                        "首次采集完成 mode="
                                        + mode
                                        + " cpu_infos="
                                        + payload.cpu_infos.Count
                                        + " gpu_infos="
                                        + payload.gpu_infos.Count
                                    );
                                    firstPayloadLogged = true;
                                }
                                // Rust 端关闭后 子进程自行退出 避免残留后台进程
                                if (!CheckIsOpen())
                                {
                                    LogInfo("检测到宿主进程已关闭 退出 OpenHardwareMonitorService");
                                    return;
                                }

                                if (!firstUploadLogged)
                                {
                                    LogInfo("准备首次上传硬件数据 mode=" + mode);
                                }
                                SendData(JsonConvert.SerializeObject(payload));
                                if (!firstUploadLogged)
                                {
                                    LogInfo("首次上传硬件数据完成 mode=" + mode);
                                    firstUploadLogged = true;
                                }
                            }
                            catch (Exception ex)
                            {
                                LogException("循环采集失败", ex);
                            }
                            Thread.Sleep(1000);
                        }
                    }
                    finally
                    {
                        computer.Close();
                    }
                }
            }
            catch (Exception ex)
            {
                LogException("OpenHardwareMonitorService 启动失败", ex);
                Environment.ExitCode = 1;
            }
        }

        private static Computer CreateComputer(MonitorMode mode)
        {
            if (mode == MonitorMode.Compatibility)
            {
                LogInfo("使用兼容模式创建 Computer CPU GPU");
                return new Computer
                {
                    CPUEnabled = true,
                    GPUEnabled = true
                };
            }

            LogInfo("使用扩展模式创建 Computer CPU GPU Mainboard RAM FanController HDD");
            return new Computer
            {
                CPUEnabled = true,
                GPUEnabled = true,
                MainboardEnabled = true,
                RAMEnabled = true,
                FanControllerEnabled = true,
                HDDEnabled = true
            };
        }

        private static HardwarePayload CollectPayload(Computer computer)
        {
            computer.Accept(UpdateVisitor);

            HardwarePayload payload = new HardwarePayload();
            foreach (IHardware hardware in computer.Hardware)
            {
                CollectHardwareRecursive(hardware, payload);
            }

            return payload;
        }

        private static void CollectHardwareRecursive(IHardware hardware, HardwarePayload payload)
        {
            HardwareInfo info = BuildHardwareInfo(hardware);
            if (hardware.HardwareType == HardwareType.CPU)
            {
                payload.cpu_infos.Add(info);
            }
            else if (hardware.HardwareType == HardwareType.GpuAti || hardware.HardwareType == HardwareType.GpuNvidia)
            {
                payload.gpu_infos.Add(info);
            }

            foreach (IHardware subHardware in hardware.SubHardware)
            {
                CollectHardwareRecursive(subHardware, payload);
            }
        }

        private static HardwareInfo BuildHardwareInfo(IHardware hardware)
        {
            HardwareInfo info = new HardwareInfo();
            foreach (ISensor sensor in hardware.Sensors)
            {
                if (!sensor.Value.HasValue)
                {
                    continue;
                }

                float value = sensor.Value.Value;
                switch (sensor.SensorType)
                {
                    case SensorType.Fan:
                        info.fans.Add(value);
                        break;
                    case SensorType.Temperature:
                        if (ContainsIgnoreCase(sensor.Name, "package"))
                        {
                            info.total_temperature = value;
                        }
                        else
                        {
                            info.temperatures.Add(value);
                        }
                        break;
                    case SensorType.Load:
                        if (ContainsIgnoreCase(sensor.Name, "total") || ContainsIgnoreCase(sensor.Name, "core max"))
                        {
                            info.total_load = value;
                        }
                        else if (ContainsIgnoreCase(sensor.Name, "memory"))
                        {
                            info.memory_loads.Add(value);
                        }
                        else
                        {
                            info.loads.Add(value);
                        }
                        break;
                    case SensorType.Clock:
                        if (!ContainsIgnoreCase(sensor.Name, "bus") && !ContainsIgnoreCase(sensor.Name, "memory"))
                        {
                            info.clocks.Add(value);
                        }
                        break;
                    case SensorType.Power:
                        if (ContainsIgnoreCase(sensor.Name, "package") || ContainsIgnoreCase(sensor.Name, "gpu") || ContainsIgnoreCase(sensor.Name, "total"))
                        {
                            info.package_power = Math.Max(info.package_power, value);
                        }
                        break;
                    case SensorType.Data:
                        if (ContainsIgnoreCase(sensor.Name, "memory") && value > 0)
                        {
                            info.memory_total = Math.Max(info.memory_total, value);
                        }
                        break;
                    default:
                        if (string.Equals(sensor.SensorType.ToString(), "SmallData", StringComparison.OrdinalIgnoreCase)
                            && ContainsIgnoreCase(sensor.Name, "memory")
                            && value > 0)
                        {
                            info.memory_total = Math.Max(info.memory_total, value);
                        }
                        break;
                }
            }

            return info;
        }

        private static bool CheckIsOpen()
        {
            try
            {
                using (HttpRequestMessage request = new HttpRequestMessage(HttpMethod.Get, _baseUrl + "isOpen"))
                {
                    using (HttpResponseMessage response = HttpClient.SendAsync(request).Result)
                    {
                        if (!response.IsSuccessStatusCode)
                        {
                            return false;
                        }

                        string content = response.Content.ReadAsStringAsync().Result;
                        return string.Equals(content.Trim(), "true", StringComparison.OrdinalIgnoreCase);
                    }
                }
            }
            catch (Exception ex)
            {
                LogException("检测宿主状态失败", ex);
                return false;
            }
        }

        private static void SendData(string jsonData)
        {
            try
            {
                using (StringContent content = new StringContent(jsonData, Encoding.UTF8, "application/json"))
                using (HttpRequestMessage request = new HttpRequestMessage(HttpMethod.Post, _baseUrl + "upload"))
                {
                    request.Content = content;
                    using (HttpResponseMessage response = HttpClient.SendAsync(request).Result)
                    {
                        if (!response.IsSuccessStatusCode)
                        {
                            LogInfo("上传数据失败 status=" + response.StatusCode);
                        }
                    }
                }
            }
            catch (Exception ex)
            {
                LogException("上传监控数据失败", ex);
            }
        }

        private static MonitorMode ParseMonitorMode(string[] args)
        {
            for (int i = 1; i < args.Length; i++)
            {
                if (string.Equals(args[i], "compat", StringComparison.OrdinalIgnoreCase))
                {
                    return MonitorMode.Compatibility;
                }
            }

            return MonitorMode.Extended;
        }

        private static void InitializeLogging()
        {
            _logPath = Path.Combine(AppDomain.CurrentDomain.BaseDirectory, "OpenHardwareMonitorService.log");
            try
            {
                File.WriteAllText(_logPath, string.Empty, Encoding.UTF8);
            }
            catch
            {
                _logPath = string.Empty;
            }
        }

        private static void OnUnhandledException(object sender, UnhandledExceptionEventArgs e)
        {
            Exception ex = e.ExceptionObject as Exception;
            LogException("未处理异常", ex);
        }

        private static void OnUnobservedTaskException(object sender, UnobservedTaskExceptionEventArgs e)
        {
            LogException("任务异常", e.Exception);
            e.SetObserved();
        }

        private static void LogInfo(string message)
        {
            string line = DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss.fff") + " INFO " + message;
            try
            {
                Console.WriteLine(line);
            }
            catch
            {
            }

            if (!string.IsNullOrEmpty(_logPath))
            {
                try
                {
                    File.AppendAllText(_logPath, line + Environment.NewLine, Encoding.UTF8);
                }
                catch
                {
                }
            }
        }

        private static void LogException(string title, Exception ex)
        {
            StringBuilder builder = new StringBuilder();
            builder.Append(DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss.fff"));
            builder.Append(" ERROR ");
            builder.Append(title);
            if (ex != null)
            {
                builder.Append(" ");
                builder.Append(ex);
            }

            string line = builder.ToString();
            try
            {
                Console.Error.WriteLine(line);
            }
            catch
            {
            }

            if (!string.IsNullOrEmpty(_logPath))
            {
                try
                {
                    File.AppendAllText(_logPath, line + Environment.NewLine, Encoding.UTF8);
                }
                catch
                {
                }
            }
        }

        private static bool ContainsIgnoreCase(string source, string value)
        {
            return source != null && source.IndexOf(value, StringComparison.OrdinalIgnoreCase) >= 0;
        }
    }
}
