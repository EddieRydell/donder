//! Stream test patterns to the setup's streamed controllers, one standard
//! input line at a time, so wiring can be checked from a terminal.
use std::io::BufRead;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use donder_model::{Controller, Layout, Setup};
use donder_output::{OutputTest, OutputTransports, TestPattern, TestSelection};
use donder_project_io::ProjectSession;

const TICK: Duration = Duration::from_millis(25);

pub(crate) const HELP: &str = "Each line: PATTERN TARGET... | stop | quit
  PATTERN: count, order, chase, rise, red, green, blue, white, or #rrggbb
  TARGET:  a fixture or group of the setup's layout, a controller,
           CONTROLLER@PORT for one of its ports,
           or CONTROLLER:FIRST:COUNT for channels counted from 1;
           a leading - runs that target from its last pixel to its first";

pub(crate) fn run(session: &ProjectSession) -> Result<(), String> {
    let (lines, commands) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if lines.send(line).is_err() {
                break;
            }
        }
    });
    eprintln!("{HELP}");
    let mut active: Option<(OutputTest, OutputTransports, Instant)> = None;
    loop {
        match commands.recv_timeout(if active.is_some() {
            TICK
        } else {
            Duration::from_secs(3600)
        }) {
            Ok(line) => {
                let words = line.split_whitespace().collect::<Vec<_>>();
                match words.as_slice() {
                    [] => continue,
                    ["quit"] => break,
                    ["stop"] => {
                        stop(&mut active)?;
                        eprintln!("stopped");
                    }
                    [pattern, targets @ ..] => match test(session, pattern, targets) {
                        Ok(test) => {
                            stop(&mut active)?;
                            let ids = test.controllers().keys().copied().collect::<Vec<_>>();
                            let transports = OutputTransports::open(test.controllers(), &ids)
                                .map_err(|error| format!("Output could not start: {error}"))?;
                            eprintln!("testing {} on {} controller(s)", line.trim(), ids.len());
                            active = Some((test, transports, Instant::now()));
                        }
                        Err(error) => eprintln!("{error}"),
                    },
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
        if let Some((test, transports, started)) = &mut active {
            transports
                .send(&test.frames(started.elapsed()))
                .map_err(|error| format!("Output failed: {error}"))?;
        }
    }
    stop(&mut active)
}

fn stop(active: &mut Option<(OutputTest, OutputTransports, Instant)>) -> Result<(), String> {
    match active.take() {
        Some((_, mut transports, _)) => transports
            .blackout_and_terminate()
            .map_err(|error| format!("Could not send blackout or terminate output: {error}")),
        None => Ok(()),
    }
}

fn test(session: &ProjectSession, pattern: &str, targets: &[&str]) -> Result<OutputTest, String> {
    let pattern = parse_pattern(pattern)?;
    if targets.is_empty() {
        return Err(format!("Name a target.\n{HELP}"));
    }
    let mut combined: Option<OutputTest> = None;
    for target in targets {
        let (reversed, target) = match target.strip_prefix('-') {
            Some(target) => (true, target),
            None => (false, *target),
        };
        let selection = selection(session, target)?;
        let mut test = OutputTest::new(&session.project, selection, pattern)
            .map_err(|error| format!("{target}: {error}"))?;
        if reversed {
            test.reverse();
        }
        match &mut combined {
            Some(combined) => combined.merge(test),
            None => combined = Some(test),
        }
    }
    combined.ok_or_else(|| "Name a target.".into())
}

fn parse_pattern(word: &str) -> Result<TestPattern, String> {
    Ok(match word {
        "count" => TestPattern::Count,
        "order" => TestPattern::Order,
        "chase" => TestPattern::Chase,
        "rise" => TestPattern::Rise,
        "red" => TestPattern::Solid([255, 0, 0]),
        "green" => TestPattern::Solid([0, 255, 0]),
        "blue" => TestPattern::Solid([0, 0, 255]),
        "white" => TestPattern::Solid([255, 255, 255]),
        hex => {
            let digits = hex
                .strip_prefix('#')
                .filter(|digits| digits.len() == 6)
                .and_then(|digits| u32::from_str_radix(digits, 16).ok())
                .ok_or_else(|| format!("Unknown pattern {hex}.\n{HELP}"))?;
            TestPattern::Solid([(digits >> 16) as u8, (digits >> 8) as u8, digits as u8])
        }
    })
}

fn selection(session: &ProjectSession, target: &str) -> Result<TestSelection, String> {
    let project = &session.project;
    let setup = project
        .get::<Setup>(*project.root().setup.id())
        .ok_or("The project's setup is missing.")?;
    if let Some((name, port)) = target.split_once('@') {
        let controller = setup_controller(session, setup, name)
            .ok_or_else(|| format!("No controller is named {name}."))?;
        let port = project
            .get::<Controller>(controller)
            .and_then(|value| {
                value
                    .ports()
                    .into_iter()
                    .find(|candidate| candidate.name == port)
            })
            .ok_or_else(|| format!("{name} has no port {port}."))?;
        return Ok(TestSelection::Port {
            controller,
            port: port.id,
        });
    }
    let (name, channels) = match target.split(':').collect::<Vec<_>>().as_slice() {
        [name] => (*name, None),
        [name, first, count] => {
            let first = first.parse::<u32>().ok().filter(|first| *first > 0);
            let count = count.parse::<u32>().ok();
            let (Some(first), Some(count)) = (first, count) else {
                return Err(format!(
                    "Write channels as {name}:FIRST:COUNT, counting from 1."
                ));
            };
            (*name, Some((first - 1, count)))
        }
        _ => return Err(format!("Unknown target {target}.")),
    };
    if let Some(controller) = setup_controller(session, setup, name) {
        let (start_channel, channel_count) = match channels {
            Some(channels) => channels,
            None => (
                0,
                project
                    .get::<Controller>(controller)
                    .and_then(Controller::universes)
                    .map_or(0, |universes| universes.channel_count()),
            ),
        };
        return Ok(TestSelection::Channels {
            controller,
            start_channel,
            channel_count,
        });
    }
    let layout = project
        .get::<Layout>(*setup.layout.id())
        .ok_or("The setup's layout is missing.")?;
    let item = layout
        .iter_fixtures()
        .find(|fixture| fixture.name.as_str() == name)
        .ok_or_else(|| format!("No controller, fixture or group is named {name}."))?;
    Ok(TestSelection::LayoutItem {
        layout: layout.id,
        item: item.id,
    })
}

/// The setup's controller declared as `name`.
fn setup_controller(
    session: &ProjectSession,
    setup: &Setup,
    name: &str,
) -> Option<donder_model::ControllerId> {
    setup
        .controllers
        .iter()
        .map(|source| *source.id())
        .find(|id| {
            session
                .address_of((*id).into())
                .is_some_and(|address| address.declared_name() == name)
        })
}
