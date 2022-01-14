import { deployHomeImplementation, deployImplementations, deployReplicaImplementation } from '../../src/upgrade';
import * as alfajores from '../../config/testnets/alfajores';
//import * as gorli from '../../config/testnets/gorli';
import * as kovan from '../../config/testnets/kovan';
//import * as mumbai from '../../config/testnets/mumbai';
//import * as fuji from '../../config/testnets/fuji';
import { CoreDeploy } from '../../src/core/CoreDeploy';

let alfajoresConfig = alfajores.devConfig;
//let gorliConfig = gorli.devConfig;
let kovanConfig = kovan.devConfig;
//let mumbaiConfig = mumbai.devConfig;
//let fujiConfig = fuji.devConfig;

const alfajoresDeploy = CoreDeploy.fromDirectory('../../rust/config/dev/', alfajores.chain, alfajoresConfig)
//const gorliDeploy = CoreDeploy.fromDirectory('../../rust/config/dev/', gorli.chain, gorliConfig)
const kovanDeploy = CoreDeploy.fromDirectory('../../rust/config/dev/', kovan.chain, kovanConfig)
//const mumbaiDeploy = CoreDeploy.fromDirectory('../../rust/config/dev/', mumbai.chain, mumbaiConfig)
//const fujiDeploy = CoreDeploy.fromDirectory('../../rust/config/dev/', fuji.chain, fujiConfig)

//const deploys = [alfajoresDeploy, gorliDeploy, kovanDeploy, mumbaiDeploy, fujiDeploy]
const deploys = [alfajoresDeploy, kovanDeploy]
async function main() {
  await deployImplementations(deploys, deployHomeImplementation)
  await deployImplementations(deploys, deployReplicaImplementation)
}
main().then(console.log).catch(console.error)
