using Newtonsoft.Json;
using OpenHardwareMonitor.Hardware;
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Net.Http;
using System.Reflection;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Forms;

namespace OpenHardwareMonitorService
{
    public class UpdateVisitor : IVisitor
    {
        public void VisitComputer(IComputer computer)
        {
            computer.Traverse(this);
        }
        public void VisitHardware(IHardware hardware)
        {
            hardware.Update();
            foreach (IHardware subHardware in hardware.SubHardware) subHardware.Accept(this);
        }
        public void VisitSensor(ISensor sensor) { }
        public void VisitParameter(IParameter parameter) { }
    }

    public class HardwareInfo
    {
        public readonly List<float> fans = new List<float>();
        public readonly List<float> temperatures = new List<float>();
        public readonly List<float> loads = new List<float>();
        public readonly List<float> clocks = new List<float>();
        public readonly List<float> powers = new List<float>();
        public float package_power = 0;
        public float cores_power = 0;
        public float total_load = 0;
        public float total_temperature = 0;
        public float memory_load = 0;
        public float memory_total = 0;
    }

    internal static class Program
    {
        static string BaseUrl = "http://localhost/";
        private static readonly HttpClient httpClient = new HttpClient();

        static Program()
        {
            //内嵌OpenHardwareMonitor的DLL
            AppDomain.CurrentDomain.AssemblyResolve += CurrentDomain_AssemblyResolve;
        }

        private static Assembly CurrentDomain_AssemblyResolve(object sender, ResolveEventArgs e)
        {
            string _resName = "OpenHardwareMonitorService.libs." + new AssemblyName(e.Name).Name + ".dll";
            Console.WriteLine("_resName:" + _resName);
            using (var _stream = Assembly.GetExecutingAssembly().GetManifestResourceStream(_resName))
            {
                byte[] _data = new byte[_stream.Length];
                _stream.Read(_data, 0, _data.Length);
                return Assembly.Load(_data);
            }
        }

        //private static void WriteLog(string message)

        //{

        //    string logFile = "LogFile.txt";

        //    File.AppendAllText(logFile, DateTime.Now.ToString() + ": " + message + Environment.NewLine);

        //}

        static void Main(string[] args)
        {
            if(args.Length > 0)
            {
                BaseUrl = "http://localhost:"+args[0]+"/";
            }
            foreach (string arg in args)
            {
                Console.WriteLine("参数:" + args[0]);
            }

            System.Threading.Mutex mutex = new System.Threading.Mutex(true, Application.ProductName, out bool ret);
            if (!ret)
            {
                MessageBox.Show("OpenHardwareMonitorService经运行!");
                Application.Exit();
                return;
            }

            //开始监测硬件
            UpdateVisitor updateVisitor = new UpdateVisitor();
            Computer computer = new Computer();
            computer.Open();
            computer.CPUEnabled = true;
            computer.GPUEnabled = true;

            while (true)
            {
                computer.Accept(updateVisitor);

                var cpu_infos = new List<HardwareInfo>();
                var gpu_infos = new List<HardwareInfo>();

                foreach (var hardware in computer.Hardware)
                {
                    var hardware_info = new HardwareInfo();

                    foreach (var sensor in hardware.Sensors)
                    {
                        if (sensor.SensorType == SensorType.Temperature)
                        {
                            if (sensor.Name.Contains("Package"))
                            {
                                hardware_info.total_temperature = sensor.Value.Value;
                            }
                            else
                            {
                                hardware_info.temperatures.Add(sensor.Value.Value);
                            }
                        }
                        else if(sensor.SensorType == SensorType.Control){
                        }
                        else if (sensor.SensorType == SensorType.Fan)
                        {
                            hardware_info.fans.Add(sensor.Value.Value);
                        }
                        else if (sensor.SensorType == SensorType.Clock)
                        {
                            //Console.WriteLine("sensor.Name=" + sensor.Name);
                            //Console.WriteLine("sensor.SensorType=" + sensor.SensorType);
                            if (sensor.Name.Contains("Bus"))
                            {
                                continue;
                            }
                            if (sensor.Name.Contains("Memory"))
                            {
                                continue;
                            }
                            if (sensor.Name.Contains("Shader")){
                                continue;
                            }
                            hardware_info.clocks.Add(sensor.Value.Value);
                        }
                        else if (sensor.SensorType == SensorType.Load)
                        {
                            if (sensor.Name.Contains("Total"))
                            {
                                hardware_info.total_load = sensor.Value.Value;
                            }
                            else if(sensor.Name.Contains("Core"))
                            {
                                hardware_info.loads.Add(sensor.Value.Value);
                            }else if(sensor.Name.Contains("Memory")){
                                hardware_info.memory_load = sensor.Value.Value;
                            }
                        }else if(sensor.SensorType == SensorType.Power)
                        {
                            if (sensor.Name.Contains("Package"))
                            {
                                hardware_info.package_power = sensor.Value.Value;
                            }else if (sensor.Name.Contains("Cores"))
                            {
                                hardware_info.cores_power = sensor.Value.Value;
                            }
                            else
                            {
                                hardware_info.powers.Add(sensor.Value.Value);
                            }
                        }else if(sensor.SensorType == SensorType.SmallData){
                            if(sensor.Name == "GPU Memory Total"){
                                hardware_info.memory_total = sensor.Value.Value;   
                            }
                        }
                    }

                    if (hardware.HardwareType == HardwareType.CPU)
                    {
                        cpu_infos.Add(hardware_info);
                    }

                    if (hardware.HardwareType == HardwareType.GpuAti || hardware.HardwareType == HardwareType.GpuNvidia)
                    {
                        gpu_infos.Add(hardware_info);
                    }
                }

                var jsonData = JsonConvert.SerializeObject(new { cpu_infos, gpu_infos });

                //Console.WriteLine(jsonData);
                //Thread.Sleep(1000*10);

                // 发送数据到Rust
                /*
                1、调用http://localhost/isOpen 返回true继续，超时或返回false则结束进程
                2、调用http://localhost/upload 发送cpu、gpu的json数据
                 */
                if (!CheckIsOpen())
                {
                    Console.WriteLine("服务不可用，结束进程...");
                    computer.Close();
                    Application.Exit();
                    return;
                }

                SendData(jsonData);

                Thread.Sleep(1000);
            }
        }

        private static bool CheckIsOpen()
        {
            try
            {
                using (var httpClient = new HttpClient())
                {
                    httpClient.Timeout = TimeSpan.FromSeconds(3);

                    var request = new HttpRequestMessage(HttpMethod.Get, $"{BaseUrl}isOpen");

                    var result = httpClient.SendAsync(request).Result;

                    if (result.StatusCode == System.Net.HttpStatusCode.OK)
                    {
                        string content = result.Content.ReadAsStringAsync().Result;
                        return content.Trim().ToLower() == "true";
                    }
                }
            }catch (Exception ex)
            {
                Debug.WriteLine(ex);
            }
            return false;
        }

        private static void SendData(string jsonData)
        {
            try
            {
                using (var httpClient = new HttpClient())
                {
                    httpClient.Timeout = TimeSpan.FromSeconds(3);

                    var content = new StringContent(jsonData, Encoding.UTF8, "application/json");
                    var request = new HttpRequestMessage(HttpMethod.Post, $"{BaseUrl}upload") { Content = content };

                    var response = httpClient.SendAsync(request).Result;
                    if (response.IsSuccessStatusCode)
                    {
                        Console.WriteLine("数据上传成功！");
                    }
                    else
                    {
                        Console.WriteLine($"上传数据失败，HTTP状态码：{response.StatusCode}");
                    }
                }
            }
            catch(Exception ex) { Debug.WriteLine(ex); }
        }
    }
}
