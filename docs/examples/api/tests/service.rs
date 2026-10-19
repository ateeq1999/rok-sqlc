//! Service tests: the business rules in `service.rs`, called directly,
//! over a database `#[sqlx::test]` creates and migrates for each test.

mod support;

use rok_sqlc_api_example::error::AppError;
use rok_sqlc_api_example::features::projects::dto::CreateProjectDto;
use rok_sqlc_api_example::features::projects::{ProjectsService, ProjectsServiceApi};
use rok_sqlc_api_example::features::tasks::dto::CreateTaskDto;
use rok_sqlc_api_example::features::tasks::{TasksService, TasksServiceApi};
use sqlx::PgPool;

use support::{TestResult, caller};

#[sqlx::test]
async fn a_new_task_counts_as_open_on_its_project(pool: PgPool) -> TestResult {
    let projects = ProjectsService::new(pool.clone());
    let tasks = TasksService::new(pool.clone());
    let alice = caller();

    let project = projects
        .create(
            alice,
            CreateProjectDto {
                name: "Launch".to_owned(),
                open_tasks: None,
                created_at: None,
            },
        )
        .await?;
    tasks
        .create(
            alice,
            CreateTaskDto {
                project_id: project.id,
                title: "Write the guide".to_owned(),
                done: None,
                created_at: None,
            },
        )
        .await?;

    let project = projects.get_one(alice, project.id).await?;
    assert_eq!(project.open_tasks, 1);
    Ok(())
}

#[sqlx::test]
async fn a_task_goes_only_into_the_callers_project(pool: PgPool) -> TestResult {
    let projects = ProjectsService::new(pool.clone());
    let tasks = TasksService::new(pool.clone());
    let (alice, bob) = (caller(), caller());

    let project = projects
        .create(
            alice,
            CreateProjectDto {
                name: "Alice's".to_owned(),
                open_tasks: None,
                created_at: None,
            },
        )
        .await?;
    let result = tasks
        .create(
            bob,
            CreateTaskDto {
                project_id: project.id,
                title: "Not mine".to_owned(),
                done: None,
                created_at: None,
            },
        )
        .await;

    assert!(matches!(result, Err(AppError::NotFound(_))));
    let project = projects.get_one(alice, project.id).await?;
    assert_eq!(project.open_tasks, 0);
    Ok(())
}
