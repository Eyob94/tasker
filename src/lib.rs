#![allow(unused)]
use std::{collections::BinaryHeap, future::Future, pin::Pin, sync::Arc, time::Duration};

use anyhow::anyhow;
use tokio::sync::{Mutex, Semaphore};
use uuid::Uuid;

#[derive(Eq, PartialEq, Debug, Ord, PartialOrd, Clone, Copy)]
pub enum Priority {
    High = 3,
    Medium = 2,
    Low = 1,
}

type JobType =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = anyhow::Result<()>> + Send>> + Send + Sync>;

pub struct Task {
    id: uuid::Uuid,
    max_retries: u32,
    priority: Priority,
    job: JobType,
}

impl Eq for Task {}

impl PartialEq for Task {
    fn eq(&self, other: &Self) -> bool {
        self.priority == other.priority
    }
}

impl PartialOrd for Task {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.priority.cmp(&other.priority))
    }
}

impl Ord for Task {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap()
    }
}

pub struct TaskManager {
    tasks: Arc<Mutex<BinaryHeap<Task>>>,
    semaphore: Arc<Semaphore>,
    result_tx: tokio::sync::mpsc::UnboundedSender<(uuid::Uuid, anyhow::Result<()>)>,
}

impl TaskManager {
    pub fn new(
        max_concurrent: usize,
        result_tx: tokio::sync::mpsc::UnboundedSender<(uuid::Uuid, anyhow::Result<()>)>,
    ) -> Self {
        Self {
            tasks: Arc::new(Mutex::new(BinaryHeap::new())),
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            result_tx,
        }
    }

    pub async fn add_task(&self, task: Task) {
        self.tasks.lock().await.push(task);
    }

    pub async fn run(&self) -> anyhow::Result<()> {
        loop {
            let semaphore = self.semaphore.clone();
            let tasks = self.tasks.clone();
            let result_tx = self.result_tx.clone();
            if self.tasks.lock().await.is_empty() {
                break;
            }
            tokio::spawn(async move {
                let task = {
                    let mut queue = tasks.lock().await;
                    queue.pop()
                };
                if task.is_none() {
                    return;
                }

                let task = task.unwrap();

                let permit = semaphore.acquire().await.unwrap();
                let mut attempts = 0;
                let mut result: anyhow::Result<()> = Ok(());

                while attempts <= task.max_retries {
                    let job = (task.job)();

                    match job.await {
                        Ok(_) => {
                            break;
                        }
                        Err(e) => {
                            attempts += 1;
                            std::thread::sleep(Duration::from_secs(2_u64.pow(attempts).max(12)));
                            result = Err(anyhow!(format!(
                                "Error running job:\n Attempt :{}\nError:{}",
                                attempts, e
                            )))
                        }
                    }
                }

                drop(permit);
                if let Err(e) = result_tx.send((task.id, result)) {
                    eprintln!("Error sending through the channel: {}", e);
                }
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {

    use super::*;

    #[tokio::test]
    async fn task_manager_works() {
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        let tm = TaskManager::new(1, result_tx);

        let new_task = Task {
            priority: Priority::High,
            max_retries: 3,
            id: Uuid::now_v7(),
            job: Arc::new(|| {
                Box::pin(async move {
                    let res = reqwest::get("https://jsonplaceholder.typicode.com/todos/1")
                        .await?
                        .text()
                        .await?;
                    println!("{res}");
                    Ok(())
                })
            }),
        };

        let new_task_2 = Task {
            id: Uuid::now_v7(),
            priority: Priority::Medium,
            max_retries: 2,
            job: Arc::new(|| {
                Box::pin(async move {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    println!("Medium");
                    Ok(())
                })
            }),
        };

        let new_task_3 = Task {
            id: Uuid::now_v7(),
            priority: Priority::Low,
            max_retries: 2,
            job: Arc::new(|| {
                Box::pin(async move {
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    println!("Low");
                    Err(anyhow!("This fails"))
                })
            }),
        };

        tm.add_task(new_task).await;
        tm.add_task(new_task_2).await;
        tm.add_task(new_task_3).await;

        tm.run().await;

        let mut errored = 0;
        let mut success = 0;
        while let Some(res) = result_rx.recv().await {
            if res.1.is_ok() {
                success += 1;
            }
            if res.1.is_err() {
                errored += 1;
            }
            if errored + success == 3 {
                break;
            }
        }

        assert!(success == 2 && errored == 1)
    }
}
