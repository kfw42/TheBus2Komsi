mod api;
mod api2vehicle;
mod komsi;
mod opts;
mod serial;
mod vehicle;

use std::io::{self, Read, Write};
use std::thread;
use std::thread::sleep;
use std::time::{Duration, Instant};
use structopt::StructOpt;

use configparser::ini::Ini;

use crate::api::getapidata;
use crate::api2vehicle::get_vehicle_state_from_api;
use crate::opts::Opts;
use crate::serial::show_serial_comports;
use crate::vehicle::compare_vehicle_states;
use crate::vehicle::init_vehicle_state;
use crate::vehicle::print_vehicle_state;

// Datei-Logging nur aktiv, wenn Feature "enablefilelogging" aktiviert ist
#[cfg(feature = "enablefilelogging")]
use std::fs::OpenOptions;

fn main() {
    let opts = Opts::from_args();

    if opts.list {
        show_serial_comports();
        return;
    }

    // default, wenn keine anderen Optionen ausgewählt,
    real_main(&opts);
}

fn real_main(opts: &Opts) {

    
    let debug = opts.debug;
    let debug_serial = opts.debug_serial;
    let debug_command = opts.debug_command;
    let verbose = opts.verbose;

    #[cfg(feature = "disablekomsiport")]
    let verbose = true;

    #[cfg(feature = "enablefilelogging")]
    let verbose = true;

    // if debug_command {
    //     let verbose = true;
    // }

    if verbose {
        println!("Verbose Mode enabled.");
    }

    let mut vehicle_state = init_vehicle_state();
    let mut api_state = -1;


    // TODO checking for file not found and elements not found
    // now we get config ini
    let mut config = Ini::new();
    let _ = config.load("TheBus2Komsi.ini");


    let baudrate = config.getint("default", "baudrate").unwrap().unwrap() as u32;
    let sleeptime = config.getint("default", "sleeptime").unwrap().unwrap() as u64;
    let portname = config.get("default", "portname").unwrap();
    let clientip = config.get("default", "ip").unwrap();

    #[cfg(not(feature = "disablekomsiport"))]
    let mut port = serialport::new(&portname, baudrate)
        .open()
        .expect("Failed to open serial port");

    #[cfg(not(feature = "disablekomsiport"))]
    if verbose {
        eprintln!("Port {:?} geöffnet mit {} baud.", &portname, &baudrate);
    }

    println!("TheBus2Komsi has started. Have fun!");

    // Datei-Logging initialisieren (nur bei aktiviertem Feature)
    #[cfg(feature = "enablefilelogging")]
    let filename = "log.txt";

    #[cfg(feature = "enablefilelogging")]
    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(filename)
        .expect(&format!("Couldn't open file {}!", filename));

    #[cfg(feature = "enablefilelogging")]
    println!("logging cmdbuf into file {}", filename);    

    // send SimulatorType:TheBus
    let string = "O1\x0a";
    let buffer = string.as_bytes();
    #[cfg(not(feature = "disablekomsiport"))]
    let _ = port.write(buffer);

    // Schreiben des Buffers in Datei, wenn Feature aktiv ist
    #[cfg(feature = "enablefilelogging")]
    {
    let buf_str = std::str::from_utf8(buffer).expect("Invalid UTF-8");
    let buf_str = &buf_str[..buf_str.len() - 1];
    writeln!(log_file, "BUFFER: {:?} <{}>", buffer, buf_str).expect("Konnte in log.txt nicht schreiben");
    }

    #[cfg(not(feature = "disablekomsiport"))]
    // Clone the port
    let mut clone = port.try_clone().expect("Failed to clone");

    // empfang über seriell ist ausgelagert in eigenen thread
    #[cfg(not(feature = "disablekomsiport"))]
    thread::spawn(move || loop {
        // Read the bytes back from the cloned port
        let mut buffer: [u8; 1] = [0; 1];

        if clone.bytes_to_read().unwrap() > 0 {
            if debug_serial {
                eprint!("REC: ");
            }

            while clone.bytes_to_read().unwrap() > 0 {
                match clone.read(&mut buffer) {
                    Ok(bytes) => {
                        if bytes > 0 {
                            if debug_serial {
                                eprint!("{}", buffer[0] as char);
                            }
                        }
                    }
                    Err(ref e) if e.kind() == io::ErrorKind::TimedOut => (),
                    Err(e) => eprintln!("{:?}", e),
                }
            }
            if debug_serial {
                eprintln!("");
            }
        }

        thread::sleep(Duration::from_millis(100));
    });

    let interval = Duration::from_millis(sleeptime);
    let mut next_time = Instant::now() + interval;

    loop {
        let api_bus_result = getapidata(&clientip, opts.debug);

        if api_bus_result.is_err() {
            if debug {
             eprintln!("getapidata error: {}", api_bus_result.unwrap_err());
            }
            if api_state != 0 {
                if verbose {
                    println!("Bitte einsteigen und hinsetzen.");
                }
                api_state = 0;
            }
        } else {
            let api_bus = api_bus_result.unwrap();
            // println!("{:?}", api_bus);
            if api_state != 1 {
                if verbose {
                    println!("Hingesetzt. Jetzt gehts los!");
                }
                api_state = 1;
            }

            let newstate = get_vehicle_state_from_api(api_bus);
            if debug {
                print_vehicle_state(&newstate);
            }

            // compare and create cmd buf
            let cmdbuf = compare_vehicle_states(&vehicle_state, &newstate, verbose, false);

            // replace after compare for next round
            vehicle_state = newstate;

            // Schreiben von cmdbuf in Datei, wenn Feature aktiv ist
            #[cfg(feature = "enablefilelogging")]
            if !cmdbuf.is_empty() {
                let buf_slice = cmdbuf.as_slice();
                let buf_str = std::str::from_utf8(buf_slice).expect("Invalid UTF-8");
                let buf_str = &buf_str[..buf_str.len() - 1];
                            writeln!(log_file, "CMDBUF: {:?} <{}>", cmdbuf, buf_str).expect(&format!("Coudn't write in file {}!", filename));
            }

            #[cfg(not(feature = "disablekomsiport"))]
            if cmdbuf.len() > 0 {
                if opts.debug_serial {
                    println!("SENDING -> {:?}", cmdbuf);
                }

                // Write to serial port
                let _ = port.write(&cmdbuf);
            }

        }

        sleep(next_time - Instant::now());
        next_time += interval;
    }
}
