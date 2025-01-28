This is a simple Async TaskManager that can manage async tasks based on priority and limiting resources through a semaphore

### Features

- priority-based-scheduling: `High`, `Medium`, and `Low` can be used to assign priorities to tasks
- Concurrency control: Limit number of tasks running at a given time by utilizing a semaphore
- Retry Mechanism: Automatic retries with exponential backoff (max 16 seconds between retries)
- Async tasks: Works with Async using Tokio

```rust
        // Channel to receive results
        let (result_tx, mut result_rx) = tokio::sync::mpsc::unbounded_channel();

        // Only 2 concurrent task at any time
        let tm = TaskManager::new(2, result_tx);

        let new_task = Task {
            priority: Priority::High,
            max_retries: 3,
            id: Uuid::now_v7(),
            job: Arc::new(|| {
                Box::pin(async move {
                    // Perform a network request
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
                    // Perform a long-running task
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
                    // Simulate another long-running task
                    tokio::time::sleep(Duration::from_secs(3)).await;
                    println!("Low");
                    // Error out to for the automatic retries
                    Err(anyhow!("This fails"))
                })
            }),
        };

        // Add tasks
        tm.add_task(new_task).await;
        tm.add_task(new_task_2).await;
        tm.add_task(new_task_3).await;

        // Start the manager
        tm.run().await;

        // Track errors and successes
        let mut errored = 0;
        let mut success = 0;

        // Receive results through the channel
        while let Some(res) = result_rx.recv().await {
            if res.1.is_ok() {
                success += 1;
            }
            if res.1.is_err() {
                errored += 1;
            }
            // If all tasks finish break
            if errored + success == 3 {
                break;
            }
        }

        // Only 2 tasks succeed
        assert!(success == 2 && errored == 1)

```

### Potential improvements

- Classifying tasks as CPU/IO bound could help dedicate workers on tokio to work on specific tasks to minimize contention.
- Better logging, simple using tracing in some areas could help with insights into how tasks are behaving, can help with debugging
