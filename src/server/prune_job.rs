use std::sync::Arc;

use anyhow::Error;

use proxmox_sys::{task_log, task_warn};

use pbs_api_types::{Authid, Operation, PruneOptions, PRIV_DATASTORE_MODIFY};
use pbs_config::CachedUserInfo;
use pbs_datastore::prune::compute_prune_info;
use pbs_datastore::DataStore;
use proxmox_rest_server::WorkerTask;

use crate::server::jobstate::Job;

pub fn prune_datastore(
    worker: Arc<WorkerTask>,
    auth_id: Authid,
    prune_options: PruneOptions,
    store: &str,
    datastore: Arc<DataStore>,
    dry_run: bool,
) -> Result<(), Error> {
    task_log!(worker, "Starting datastore prune on store \"{}\"", store);

    if dry_run {
        task_log!(worker, "(dry test run)");
    }

    let keep_all = !pbs_datastore::prune::keeps_something(&prune_options);

    if keep_all {
        task_log!(worker, "No prune selection - keeping all files.");
    } else {
        task_log!(
            worker,
            "retention options: {}",
            pbs_datastore::prune::cli_options_string(&prune_options)
        );
    }

    let user_info = CachedUserInfo::new()?;
    let privs = user_info.lookup_privs(&auth_id, &["datastore", store]);
    let has_privs = privs & PRIV_DATASTORE_MODIFY != 0;

    // FIXME: Namespaces and recursion!
    for group in datastore.iter_backup_groups(Default::default())? {
        let group = group?;
        let list = group.list_backups()?;

        if !has_privs && !datastore.owns_backup(group.as_ref(), &auth_id)? {
            continue;
        }

        let mut prune_info = compute_prune_info(list, &prune_options)?;
        prune_info.reverse(); // delete older snapshots first

        task_log!(
            worker,
            "Starting prune on store \"{}\" group \"{}/{}\"",
            store,
            group.backup_type(),
            group.backup_id()
        );

        for (info, mark) in prune_info {
            let keep = keep_all || mark.keep();
            task_log!(
                worker,
                "{} {}/{}/{}",
                mark,
                group.backup_type(),
                group.backup_id(),
                info.backup_dir.backup_time_string()
            );
            if !keep && !dry_run {
                if let Err(err) = datastore.remove_backup_dir(info.backup_dir.as_ref(), false) {
                    task_warn!(
                        worker,
                        "failed to remove dir {:?}: {}",
                        info.backup_dir.relative_path(),
                        err,
                    );
                }
            }
        }
    }

    Ok(())
}

pub fn do_prune_job(
    mut job: Job,
    prune_options: PruneOptions,
    store: String,
    auth_id: &Authid,
    schedule: Option<String>,
) -> Result<String, Error> {
    let datastore = DataStore::lookup_datastore(&store, Some(Operation::Write))?;

    let worker_type = job.jobtype().to_string();
    let auth_id = auth_id.clone();
    let upid_str = WorkerTask::new_thread(
        &worker_type,
        Some(job.jobname().to_string()),
        auth_id.to_string(),
        false,
        move |worker| {
            job.start(&worker.upid().to_string())?;

            if let Some(event_str) = schedule {
                task_log!(worker, "task triggered by schedule '{}'", event_str);
            }

            let result = prune_datastore(
                worker.clone(),
                auth_id,
                prune_options,
                &store,
                datastore,
                false,
            );

            let status = worker.create_state(&result);

            if let Err(err) = job.finish(status) {
                eprintln!(
                    "could not finish job state for {}: {}",
                    job.jobtype().to_string(),
                    err
                );
            }

            result
        },
    )?;
    Ok(upid_str)
}
