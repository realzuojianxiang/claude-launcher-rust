export namespace main {
	
	export class Config {
	    work_dir: string;
	    anthropic_url: string;
	    anthropic_key: string;
	    cliproxyapi_key: string;
	
	    static createFrom(source: any = {}) {
	        return new Config(source);
	    }
	
	    constructor(source: any = {}) {
	        if ('string' === typeof source) source = JSON.parse(source);
	        this.work_dir = source["work_dir"];
	        this.anthropic_url = source["anthropic_url"];
	        this.anthropic_key = source["anthropic_key"];
	        this.cliproxyapi_key = source["cliproxyapi_key"];
	    }
	}

}

