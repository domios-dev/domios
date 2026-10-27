import {verifyQualification, selectQualificationArtifact, retainQualificationEvidence} from './ci-qualification.mjs';
import {readQualificationArchive} from './ci-qualification-archive.mjs';
export {readQualificationArchive} from './ci-qualification-archive.mjs';

export async function verifyIssueReferences(github, selection) {
  for (const issue of [...selection.issues,...selection.repository_issues]) {
    const match=/^([^/]+)\/([^#]+)#(\d+)$/.exec(issue.reference);
    if(!match)throw new Error('Invalid declared issue reference: '+issue.reference);
    await github.rest.issues.get({owner:match[1],repo:match[2],issue_number:Number(match[3])});
  }
}
export async function listFailureJobs(github, context) {
  const jobs=await github.paginate(github.rest.actions.listJobsForWorkflowRunAttempt,{
    ...context.repo,run_id:context.runId,attempt_number:Number(process.env.GITHUB_RUN_ATTEMPT),per_page:100});
  return jobs.filter(job=>job.conclusion&&!['success','skipped'].includes(job.conclusion)).map(job=>job.name+': '+job.conclusion);
}
export async function cancelWorkflow(github, context) {
  await github.rest.actions.cancelWorkflowRun({...context.repo,run_id:context.runId});
}
export async function requireQualificationWithGithub(github, context, sha, readArchive=readQualificationArchive, retain=retainQualificationEvidence) {
  if(!/^[a-f0-9]{40}$/.test(sha))throw new Error('Invalid qualification SHA');
  const name='qualification-'+sha;
  const artifacts=(await github.rest.actions.listArtifactsForRepo({...context.repo,name,per_page:100})).data.artifacts;
  const artifact=selectQualificationArtifact(artifacts,sha);
  const run=(await github.rest.actions.getWorkflowRun({...context.repo,run_id:artifact.workflow_run.id})).data;
  if(String(run.id)!==String(artifact.workflow_run.id))throw new Error('Foreign qualification artifact run');
  const jobs=[];
  for(let page=1;;page++){
    if(page>20)throw new Error('Qualification job inventory exceeds bound');
    const batch=(await github.rest.actions.listJobsForWorkflowRun({...context.repo,run_id:run.id,filter:'all',per_page:100,page})).data;
    jobs.push(...batch.jobs);if(jobs.length>=batch.total_count)break;
    if(!batch.jobs.length)throw new Error('Incomplete qualification job inventory');
  }
  const archive=(await github.rest.actions.downloadArtifact({...context.repo,artifact_id:artifact.id,archive_format:'zip'})).data;
  return retain({...verifyQualification(readArchive(archive),run,jobs,sha,context.repo.owner+'/'+context.repo.repo),
    verification_artifact:{id:artifact.id,expires_at:artifact.expires_at}});
}
