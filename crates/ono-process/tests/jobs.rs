//! Job control of spec §18.1 and cancellation of spec §18.5, on the non-terminal path.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a failed precondition in a test should abort the test loudly"
)]
mod support;

use std::thread;
use std::time::Duration;

use ono_process::{Command, Executor, ForegroundOutcome, JobState, Output, Signal};
use support::{DEADLINE, poll_until, sh, text, within};

#[test]
fn should_record_a_background_job_and_report_its_final_status() {
    within(DEADLINE, || {
        let mut executor = Executor::detached();
        let id = executor
            .run_background(&sh("exit 5").into())
            .expect("the job must start");

        let job = executor
            .wait_job(id, Some(DEADLINE))
            .expect("waiting must not fail")
            .expect("the job must finish inside the deadline");

        assert_eq!(
            job.state,
            JobState::Exited(ono_core::ExitStatus::from_code(5))
        );
        assert_eq!(job.id, id);
        assert!(job.pgid > 1, "a background job leads its own process group");
    });
}

#[test]
fn should_list_a_running_background_job_with_its_command_text() {
    within(DEADLINE, || {
        let mut executor = Executor::detached();
        let id = executor
            .run_background(&sh("read ignored").stdin(ono_process::Input::Null).into())
            .expect("the job must start");

        let jobs = executor.jobs();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, id);
        assert!(
            jobs[0].command.contains("read ignored"),
            "the job record carries the command text, got {:?}",
            jobs[0].command
        );

        executor
            .wait_job(id, Some(DEADLINE))
            .expect("waiting must not fail")
            .expect("stdin was empty, so the job finishes");
    });
}

#[test]
fn should_number_jobs_in_the_order_they_start() {
    within(DEADLINE, || {
        let mut executor = Executor::detached();
        let first = executor
            .run_background(&sh("exit 0").into())
            .expect("start");
        let second = executor
            .run_background(&sh("exit 0").into())
            .expect("start");
        assert_eq!(first.number(), 1);
        assert_eq!(second.number(), 2);
        assert_eq!(first.to_string(), "%1");
        for id in [first, second] {
            executor
                .wait_job(id, Some(DEADLINE))
                .expect("waiting must not fail")
                .expect("the job must finish");
        }
    });
}

#[test]
fn should_observe_a_stop_and_a_continue_without_losing_either_transition() {
    within(DEADLINE, || {
        let mut executor = Executor::detached();
        let id = executor
            .run_background(&sh("kill -TSTP $$; exit 3").into())
            .expect("the job must start");

        poll_until(DEADLINE, || {
            executor.poll_jobs().expect("polling must not fail");
            match executor.job(id).map(|job| job.state) {
                Some(JobState::Stopped(signal)) => Some(signal),
                _ => None,
            }
        });

        executor
            .background(id)
            .expect("continuing the job must succeed");

        let job = executor
            .wait_job(id, Some(DEADLINE))
            .expect("waiting must not fail")
            .expect("the continued job must finish");
        assert_eq!(
            job.state,
            JobState::Exited(ono_core::ExitStatus::from_code(3))
        );
    });
}

#[test]
fn should_stop_a_job_when_it_is_signalled_and_report_the_stop_signal() {
    within(DEADLINE, || {
        let mut executor = Executor::detached();
        let id = executor
            .run_background(&Command::new("sleep").arg("30").into())
            .expect("the job must start");

        executor
            .signal_job(id, Signal::TSTP)
            .expect("signalling must succeed");

        let signal = poll_until(DEADLINE, || {
            executor.poll_jobs().expect("polling must not fail");
            match executor.job(id).map(|job| job.state) {
                Some(JobState::Stopped(signal)) => Some(signal),
                _ => None,
            }
        });
        assert_eq!(signal, Signal::TSTP);

        executor
            .signal_job(id, Signal::KILL)
            .expect("signalling must succeed");
        let job = executor
            .wait_job(id, Some(DEADLINE))
            .expect("waiting must not fail")
            .expect("the killed job must finish");
        assert_eq!(
            job.state,
            JobState::Exited(ono_core::ExitStatus::from_signal(9))
        );
    });
}

#[test]
fn should_return_a_finished_background_job_to_the_foreground_with_its_status() {
    within(DEADLINE, || {
        let mut executor = Executor::detached();
        let id = executor
            .run_background(&sh("exit 9").into())
            .expect("the job must start");
        thread::sleep(Duration::from_millis(50));

        let outcome = executor.foreground(id).expect("fg must succeed");
        match outcome {
            ForegroundOutcome::Completed(outcome) => assert_eq!(outcome.status().code(), 9),
            ForegroundOutcome::Stopped { .. } => panic!("the job had already finished"),
        }
        assert!(
            executor.job(id).is_none(),
            "a finished job leaves the table once it has been reported"
        );
    });
}

#[test]
fn should_continue_a_stopped_job_when_it_is_brought_to_the_foreground() {
    within(DEADLINE, || {
        let mut executor = Executor::detached();
        let id = executor
            .run_background(&sh("kill -TSTP $$; exit 4").into())
            .expect("the job must start");

        poll_until(DEADLINE, || {
            executor.poll_jobs().expect("polling must not fail");
            matches!(
                executor.job(id).map(|job| job.state),
                Some(JobState::Stopped(_))
            )
            .then_some(())
        });

        let outcome = executor.foreground(id).expect("fg must succeed");
        match outcome {
            ForegroundOutcome::Completed(outcome) => assert_eq!(outcome.status().code(), 4),
            ForegroundOutcome::Stopped { .. } => panic!("fg must continue the stopped job"),
        }
    });
}

#[test]
fn should_report_a_foreground_job_that_stops_instead_of_blocking() {
    within(DEADLINE, || {
        let mut executor = Executor::detached();
        let outcome = executor
            .run_foreground(&sh("kill -TSTP $$; exit 6").into())
            .expect("the run must not fail");
        let id = match outcome {
            ForegroundOutcome::Stopped { job, signal } => {
                assert_eq!(signal, Signal::TSTP);
                job
            }
            ForegroundOutcome::Completed(_) => panic!("the command stopped itself"),
        };

        assert!(
            executor.job(id).is_some(),
            "a stopped foreground command becomes a job"
        );

        let outcome = executor.foreground(id).expect("fg must succeed");
        match outcome {
            ForegroundOutcome::Completed(outcome) => assert_eq!(outcome.status().code(), 6),
            ForegroundOutcome::Stopped { .. } => panic!("the job must finish this time"),
        }
    });
}

#[test]
fn should_report_a_stopped_foreground_job_as_128_plus_the_stop_signal() {
    within(DEADLINE, || {
        let mut executor = Executor::detached();
        let outcome = executor
            .run_foreground(&sh("kill -TSTP $$; exit 6").into())
            .expect("the run must not fail");
        assert_eq!(outcome.status().code(), 128 + 20, "SIGTSTP is 20 on Linux");
        if let ForegroundOutcome::Stopped { job, .. } = outcome {
            executor.signal_job(job, Signal::KILL).expect("clean up");
            executor
                .wait_job(job, Some(DEADLINE))
                .expect("waiting must not fail");
        }
    });
}

#[test]
fn should_cancel_a_running_foreground_job_with_sigint() {
    within(DEADLINE, || {
        let mut executor = Executor::detached();
        let canceller = executor.canceller();
        let waiter = thread::spawn(move || {
            poll_until(DEADLINE, || canceller.is_active().then_some(()));
            canceller.cancel().expect("cancelling must succeed");
        });

        let outcome = executor
            .run_foreground(&Command::new("sleep").arg("30").into())
            .expect("the run must not fail");
        waiter.join().expect("the canceller thread must finish");

        assert_eq!(
            outcome.status().code(),
            130,
            "a cancelled foreground job reports 128 + SIGINT (ADR-0008)"
        );
    });
}

#[test]
fn should_report_no_active_foreground_job_when_nothing_is_running() {
    let executor = Executor::detached();
    assert!(!executor.canceller().is_active());
    assert!(
        executor.canceller().cancel().is_ok(),
        "cancelling with nothing in the foreground is a no-op"
    );
}

#[test]
fn should_run_a_background_pipeline_as_one_job() {
    within(DEADLINE, || {
        let mut executor = Executor::detached();
        let pipeline = ono_process::Pipeline::new()
            .stage(Command::new("echo").arg("bg"))
            .stage(Command::new("cat").stdout(Output::Null));
        let id = executor
            .run_background(&pipeline)
            .expect("the job must start");

        let job = executor
            .wait_job(id, Some(DEADLINE))
            .expect("waiting must not fail")
            .expect("the job must finish");
        assert_eq!(job.processes.len(), 2, "both stages belong to one job");
        for process in &job.processes {
            assert_eq!(
                process.state,
                JobState::Exited(ono_core::ExitStatus::SUCCESS)
            );
        }
    });
}

#[test]
fn should_capture_the_output_of_a_background_job_when_it_is_brought_forward() {
    within(DEADLINE, || {
        let mut executor = Executor::detached();
        let id = executor
            .run_background(
                &Command::new("echo")
                    .arg("late")
                    .stdout(Output::Capture)
                    .into(),
            )
            .expect("the job must start");
        let outcome = executor.foreground(id).expect("fg must succeed");
        match outcome {
            ForegroundOutcome::Completed(outcome) => {
                assert_eq!(text(outcome.stdout()), "late\n");
            }
            ForegroundOutcome::Stopped { .. } => panic!("echo does not stop"),
        }
    });
}

#[test]
fn should_time_out_rather_than_block_when_a_job_does_not_finish() {
    within(DEADLINE, || {
        let mut executor = Executor::detached();
        let id = executor
            .run_background(&Command::new("sleep").arg("30").into())
            .expect("the job must start");
        let finished = executor
            .wait_job(id, Some(Duration::from_millis(200)))
            .expect("waiting must not fail");
        assert!(finished.is_none(), "the job is still running");
        executor
            .signal_job(id, Signal::KILL)
            .expect("signalling must succeed");
        executor
            .wait_job(id, Some(DEADLINE))
            .expect("waiting must not fail")
            .expect("the killed job must finish");
    });
}
